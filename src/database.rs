use rusqlite::{Connection, Result};

// Open and prepare Database
pub fn open_db() -> Result<Connection> {
    let db_path = "./data/data.db";
    let db = Connection::open(&db_path)?;
    db.pragma_update(None, "foreign_keys", "ON")
        .expect("Failed to set PRAGMA");
    db.pragma_update(None, "journal_mode", "WAL")
        .expect("Failed to set PRAGMA");
    db.pragma_update(None, "auto_vacuum", "FULL")
        .expect("Failed to set PRAGMA");

    db.execute(
        "CREATE TABLE IF NOT EXISTS location_history (
                  id              INTEGER PRIMARY KEY AUTOINCREMENT,
                  waypoint        TEXT NOT NULL,
                  createdAt       TEXT DEFAULT CURRENT_TIMESTAMP
                  )",
        [],
    )
    .expect("Failed when checking for history table in database");

    Ok(db)
}
