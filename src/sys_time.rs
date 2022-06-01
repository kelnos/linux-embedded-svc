use embedded_svc::sys_time::SystemTime;
use std::time::{Duration, SystemTime as StdSystemTime};

pub struct LinuxSystemTime;

impl SystemTime for LinuxSystemTime {
    fn now(&self) -> Duration {
        StdSystemTime::now().duration_since(StdSystemTime::UNIX_EPOCH).unwrap()
    }
}
