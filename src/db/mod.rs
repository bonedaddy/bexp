pub mod queries;
pub mod schema;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{Result, VexpError};

pub struct Database {
    writer: Mutex<Connection>,
    reader: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut writer = Connection::open(path)?;
        Self::configure_connection(&writer)?;
        Self::apply_schema(&mut writer)?;

        let reader = Connection::open(path)?;
        Self::configure_connection(&reader)?;

        Ok(Self {
            writer: Mutex::new(writer),
            reader: Mutex::new(reader),
        })
    }

    pub fn open_memory() -> Result<Self> {
        let mut writer = Connection::open_in_memory()?;
        Self::configure_connection(&writer)?;
        Self::apply_schema(&mut writer)?;

        let reader = Connection::open_in_memory()?;
        Self::configure_connection(&reader)?;

        Ok(Self {
            writer: Mutex::new(writer),
            reader: Mutex::new(reader),
        })
    }

    fn configure_connection(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -64000;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    }

    fn apply_schema(conn: &mut Connection) -> Result<()> {
        conn.execute_batch(schema::SCHEMA)
            .map_err(|e| VexpError::Migration(format!("Schema apply failed: {e}")))?;
        Ok(())
    }

    pub fn writer(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.writer.lock().expect("writer lock poisoned")
    }

    pub fn reader(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.reader.lock().expect("reader lock poisoned")
    }

    pub fn flush_wal(&self) -> Result<()> {
        let conn = self.writer();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
}
