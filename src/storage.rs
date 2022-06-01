use core::fmt;
use embedded_svc::{errors::Errors, storage::Storage};
use rusqlite::*;
use std::path::Path;

#[derive(Debug)]
pub enum SqliteStorageError {
    Sqlite(rusqlite::Error),
}

impl fmt::Display for SqliteStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SqliteStorageError::Sqlite(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for SqliteStorageError {}

impl From<rusqlite::Error> for SqliteStorageError {
    fn from(error: rusqlite::Error) -> Self {
        SqliteStorageError::Sqlite(error)
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
                key TEXT NOT NULL PRIMARY KEY,
                value BLOB
            )",
            [],
        )?;
        Ok(SqliteStorage { db })
    }
}

impl Errors for SqliteStorage {
    type Error = SqliteStorageError;
}

impl Storage for SqliteStorage {
    fn contains(&self, key: impl AsRef<str>) -> Result<bool, Self::Error> {
        match self.db.query_row(
            "SELECT key FROM embedded_svc_storage WHERE key = ?",
            [key.as_ref()],
            |_| Ok(()),
        ) {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    fn len(&self, key: impl AsRef<str>) -> Result<Option<usize>, Self::Error> {
        match self.db.query_row(
            "SELECT key,value FROM embedded_svc_storage WHERE key = ?",
            [key.as_ref()],
            |row| row.get::<&str, Vec<u8>>("value"),
        ) {
            Ok(value) => Ok(Some(value.len())),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn get_raw(&self, key: impl AsRef<str>) -> Result<Option<alloc::vec::Vec<u8>>, Self::Error> {
        match self.db.query_row(
            "SELECT key,value FROM embedded_svc_storage WHERE key = ?",
            [key.as_ref()],
            |row| row.get::<&str, alloc::vec::Vec<u8>>("value"),
        ) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn put_raw(
        &mut self,
        key: impl AsRef<str>,
        value: impl Into<alloc::vec::Vec<u8>>,
    ) -> Result<bool, Self::Error> {
        let res = self.db.execute(
            "INSERT INTO embedded_svc_storage VALUES(?, ?) ON CONFLICT DO UPDATE SET key=excluded.key, value=excluded.value",
            rusqlite::params![key.as_ref(), value.into()],
        )?;
        Ok(res == 1)
    }

    fn remove(&mut self, key: impl AsRef<str>) -> Result<bool, Self::Error> {
        let res = self.db.execute(
            "DELETE FROM embedded_svc_storage WHERE key = ?",
            [key.as_ref()],
        )?;
        Ok(res == 1)
    }
}
