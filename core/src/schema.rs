use anyhow::Result;
use rusqlite::Connection;

const DEFINITION: &str = "
CREATE TABLE IF NOT EXISTS agents (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    initials TEXT NOT NULL,
    role TEXT NOT NULL,
    status TEXT NOT NULL,
    status_detail TEXT,
    sort_index INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS channels (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    group_name TEXT,
    kind TEXT NOT NULL,
    agent_id INTEGER,
    unread INTEGER NOT NULL,
    sort_index INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL,
    thread_root_id INTEGER,
    author TEXT NOT NULL,
    agent_id INTEGER,
    body TEXT NOT NULL,
    sent_at TEXT NOT NULL,
    reply_count INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS seed_marker (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    seeded_at TEXT NOT NULL
);
";

pub fn create(connection: &Connection) -> Result<()> {
    connection.execute_batch(DEFINITION)?;
    Ok(())
}
