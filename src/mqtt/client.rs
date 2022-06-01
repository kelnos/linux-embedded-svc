use embedded_svc::{errors::Errors, mqtt::client::*};
use rumqttc::{Client as RumqttcClient, ClientError as RumqttcError};
use std::{
    fmt,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
};

#[derive(Debug)]
pub enum MqttError {
    Client(RumqttcError),
    Connection(rumqttc::ConnectionError),
}

impl fmt::Display for MqttError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MqttError::Client(err) => err.fmt(f),
            MqttError::Connection(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for MqttError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MqttError::Client(err) => Some(err),
            MqttError::Connection(err) => Some(err),
        }
    }
}

impl From<RumqttcError> for MqttError {
    fn from(error: RumqttcError) -> Self {
        MqttError::Client(error)
    }
}

impl From<rumqttc::ConnectionError> for MqttError {
    fn from(error: rumqttc::ConnectionError) -> Self {
        MqttError::Connection(error)
    }
}

struct QueuedMessage {
    topic: String,
    qos: rumqttc::QoS,
    retain: bool,
    payload: Vec<u8>,
}

pub struct MqttConnectionIter<'a>(pub(crate) rumqttc::ConnectionIter<'a>);

impl<'a> Errors for MqttConnectionIter<'a> {
    type Error = MqttError;
}

impl<'a> Connection for MqttConnectionIter<'a> {
    type Message = ();
    fn next(&mut self) -> Option<Result<Event<Self::Message>, Self::Error>> {
        match self.0.next() {
            Some(Ok(rumqttc::Event::Incoming(incoming))) => {
                packet_to_event(incoming).map(|ev| Ok(ev))
            }
            Some(Ok(rumqttc::Event::Outgoing(_))) => None,
            Some(Err(err)) => Some(Err(err.into())),
            None => None,
        }
    }
}

impl<'a> Iterator for MqttConnectionIter<'a> {
    type Item = Result<Event<()>, MqttError>;
    fn next(&mut self) -> Option<Self::Item> {
        Connection::next(self)
    }
}

pub struct MqttConnection(pub(crate) rumqttc::Connection);

impl<'a> MqttConnection {
    pub fn iter(&'a mut self) -> MqttConnectionIter<'a> {
        MqttConnectionIter(self.0.iter())
    }
}

pub struct MqttClient {
    inner: Arc<Mutex<RumqttcClient>>,
    queue: Option<(JoinHandle<()>, mpsc::SyncSender<QueuedMessage>)>,
}

impl MqttClient {
    pub fn new<'a>(
        options: rumqttc::MqttOptions,
        cap: usize,
        publish_queue_size: usize,
    ) -> (Self, MqttConnection) {
        let (client, conn) = RumqttcClient::new(options, cap);
        let inner = Arc::new(Mutex::new(client));
        let (tx, rx) = mpsc::sync_channel(publish_queue_size);
        let queue_thread = {
            let client = Arc::clone(&inner);
            thread::spawn(move || queue_thread(client, rx))
        };

        let mqtt_client = Self {
            inner,
            queue: Some((queue_thread, tx)),
        };
        let mqtt_conn = MqttConnection(conn);

        (mqtt_client, mqtt_conn)
    }
}

impl Drop for MqttClient {
    fn drop(&mut self) {
        if let Some((queue_thread, tx)) = self.queue.take() {
            drop(tx);
            let _ = queue_thread.join();
        }
    }
}

impl Errors for MqttClient {
    type Error = MqttError;
}

impl Client for MqttClient {
    fn subscribe<'a, S>(&'a mut self, topic: S, qos: QoS) -> Result<MessageId, Self::Error>
    where
        S: Into<std::borrow::Cow<'a, str>>,
    {
        self.inner
            .lock()
            .unwrap()
            .subscribe(topic.into(), qos_to_qos(qos))
            .map(|_| 0)
            .map_err(|err| err.into())
    }

    fn unsubscribe<'a, S>(&'a mut self, topic: S) -> Result<MessageId, Self::Error>
    where
        S: Into<std::borrow::Cow<'a, str>>,
    {
        self.inner
            .lock()
            .unwrap()
            .unsubscribe(topic.into())
            .map(|_| 0)
            .map_err(|err| err.into())
    }
}

impl Enqueue for MqttClient {
    fn enqueue<'a, S, V>(
        &'a mut self,
        topic: S,
        qos: QoS,
        retain: bool,
        payload: V,
    ) -> Result<MessageId, Self::Error>
    where
        S: Into<std::borrow::Cow<'a, str>>,
        V: Into<std::borrow::Cow<'a, [u8]>>,
    {
        if let Some((_, tx)) = &self.queue {
            let _ = tx.send(QueuedMessage {
                topic: topic.into().into_owned(),
                qos: qos_to_qos(qos),
                retain,
                payload: payload.into().into(),
            });
        }
        Ok(0)
    }
}

impl Publish for MqttClient {
    fn publish<'a, S, V>(
        &'a mut self,
        topic: S,
        qos: QoS,
        retain: bool,
        payload: V,
    ) -> Result<MessageId, Self::Error>
    where
        S: Into<std::borrow::Cow<'a, str>>,
        V: Into<std::borrow::Cow<'a, [u8]>>,
    {
        self.inner
            .lock()
            .unwrap()
            .publish(topic.into(), qos_to_qos(qos), retain, payload.into())
            .map(|_| 0)
            .map_err(|err| err.into())
    }
}

fn queue_thread(client: Arc<Mutex<RumqttcClient>>, rx: mpsc::Receiver<QueuedMessage>) {
    while let Ok(message) = rx.recv() {
        let _ = client.lock().unwrap().publish(
            message.topic,
            message.qos,
            message.retain,
            message.payload,
        );
    }
}

fn qos_to_qos(qos: QoS) -> rumqttc::QoS {
    match qos {
        QoS::AtMostOnce => rumqttc::QoS::AtMostOnce,
        QoS::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
        QoS::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
    }
}

fn packet_to_event(pkt: rumqttc::Incoming) -> Option<Event<()>> {
    use rumqttc::mqttbytes::v4::Packet::*;
    match pkt {
        ConnAck(ca) => Some(Event::Connected(ca.session_present)),
        PubAck(pa) => Some(Event::Published(pa.pkid as u32)),
        PubRel(pr) => Some(Event::Deleted(pr.pkid as u32)),
        PubComp(pc) => Some(Event::Published(pc.pkid as u32)),
        SubAck(sa) => Some(Event::Subscribed(sa.pkid as u32)),
        UnsubAck(ua) => Some(Event::Unsubscribed(ua.pkid as u32)),
        Disconnect => Some(Event::Disconnected),
        _ => None,
    }
}
