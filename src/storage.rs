use core::fmt;
use embedded_svc::storage::{StorageBase, RawStorage};
use rusqlite::*;
use std::path::Path;

#[cfg(feature = "serde")]
use embedded_svc::storage::Storage;

#[derive(Debug)]
pub enum SqliteStorageError {
    Sqlite(rusqlite::Error),
    BufferTooSmall(u64),
    #[cfg(feature = "serde")]
    Serde(serde_json::Error),
}

impl fmt::Display for SqliteStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SqliteStorageError::Sqlite(err) => write!(f, "sqlite error: {}", err),
            SqliteStorageError::BufferTooSmall(required_size) => write!(f, "provided buffer was too small (needed {} bytes)", required_size),
            #[cfg(feature = "serde")]
            SqliteStorageError::Serde(err) => write!(f, "serde error: {}", err),
        }
    }
}

impl std::error::Error for SqliteStorageError {}

impl From<rusqlite::Error> for SqliteStorageError {
    fn from(error: rusqlite::Error) -> Self {
        SqliteStorageError::Sqlite(error)
    }
}

#[cfg(feature = "serde")]
impl From<serde_json::Error> for SqliteStorageError {
    fn from(value: serde_json::Error) -> Self {
        SqliteStorageError::Serde(value)
    }
}

pub struct SqliteStorage {
    db: Connection,
}

impl SqliteStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, SqliteStorageError> {
        let db = Connection::open(path)?;
        db.execute(
            "CREATE TABLE IF NOT EXISTS embedded_svc_storage (
                name TEXT NOT NULL PRIMARY KEY,
                value BLOB
            )",
            [],
        )?;
        Ok(SqliteStorage { db })
    }

    fn get_vec(&self, name: &str) -> Result<Option<Vec<u8>>, SqliteStorageError> {
        match self.db.query_row(
            "SELECT name,value FROM embedded_svc_storage WHERE name = ?",
            [name],
            |row| row.get::<&str, alloc::vec::Vec<u8>>("value"),
        ) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

impl StorageBase for SqliteStorage {
    type Error = SqliteStorageError;

    fn contains(&self, name: &str) -> std::result::Result<bool, Self::Error> {
        match self.db.query_row(
            "SELECT name FROM embedded_svc_storage WHERE name = ?",
            [name],
            |_| Ok(()),
        ) {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    fn remove(&mut self, name: &str) -> std::result::Result<bool, Self::Error> {
        let res = self.db.execute(
            "DELETE FROM embedded_svc_storage WHERE name = ?",
            [name],
        )?;
        Ok(res == 1)
    }
}

impl RawStorage for SqliteStorage {
    fn len(&self, name: &str) -> std::result::Result<Option<usize>, Self::Error> {
        match self.db.query_row(
            "SELECT name,value FROM embedded_svc_storage WHERE name = ?",
            [name],
            |row| row.get::<&str, Vec<u8>>("value"),
        ) {
            Ok(value) => Ok(Some(value.len())),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn get_raw<'a>(&self, name: &str, buf: &'a mut [u8]) -> std::result::Result<Option<&'a [u8]>, Self::Error> {
        self.get_vec(name).map(|res| res.map(|value| {
            let retbuf = &mut buf[0..value.len()];
            retbuf.copy_from_slice(&value);
            retbuf as &[u8]
        }))
    }

    fn set_raw(&mut self, name: &str, buf: &[u8]) -> std::result::Result<bool, Self::Error> {
        let res = self.db.execute(
            "INSERT INTO embedded_svc_storage VALUES(?, ?) ON CONFLICT DO UPDATE SET name=excluded.name, value=excluded.value",
            rusqlite::params![name, buf],
        )?;
        Ok(res == 1)
    }
}

#[cfg(feature = "serde")]
impl Storage for SqliteStorage {
    fn get<T>(&self, name: &str) -> std::result::Result<Option<T>, Self::Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self
            .get_vec(name)
            .and_then(|res|
                res
                    .map(|value|
                        serde_json::from_slice::<T>(&value)
                            .map_err(Into::<SqliteStorageError>::into)
                    )
                    .transpose()
            )
    }

    fn set<T>(&mut self, name: &str, value: &T) -> std::result::Result<bool, Self::Error>
    where
        T: serde::Serialize,
    {
        let buf = serde_json::to_vec(value)?;
        self.set_raw(name, &buf)
    }
}
