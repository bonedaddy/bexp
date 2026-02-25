pub mod cross_ref;

use std::path::Path;

use rusqlite::Connection;

use crate::error::{BexpError, Result};

/// Open another workspace's bexp database in read-only mode.
pub fn open_external_db(workspace_root: &str) -> Result<Connection> {
    let db_path = Path::new(workspace_root).join(".bexp/index.db");
    if !db_path.exists() {
        return Err(BexpError::Config(format!(
            "External workspace DB not found: {}",
            db_path.display()
        )));
    }
    let conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(conn)
}
