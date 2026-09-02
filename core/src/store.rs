use std::path::Path;

use anyhow::{Result, bail};
use rusqlite::{Connection, Row};
use time::OffsetDateTime;

use crate::fixtures::{Conversation, Fixtures, SeedMessage, SeedReply, fixtures};
use crate::model::{
    Agent, AgentId, AgentStatus, Author, Channel, ChannelId, ChannelKind, Message, MessageId, Span,
    decode, encode,
};
use crate::schema;

const MESSAGE_COLUMNS: &str = "id, author, agent_id, body, sent_at, reply_count";

/// The SQLite database behind the workspace.
///
/// Every read returns domain types in an explicit order, and every write that
/// touches more than one row runs in a transaction.
pub struct Store {
    connection: Connection,
}

impl Store {
    /// Opens the database at `path`, creating the schema when it is absent.
    ///
    /// The directory holding `path` is not created.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or the schema cannot be
    /// created.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::store::Store;
    ///
    /// let path =
    ///     std::env::temp_dir().join(format!("tuclaw-doc-open-{}.sqlite", std::process::id()));
    /// let store = Store::open(&path).unwrap();
    /// assert!(store.channels().unwrap().is_empty());
    /// std::fs::remove_file(&path).unwrap();
    /// ```
    pub fn open(path: &Path) -> Result<Store> {
        let connection = Connection::open(path)?;
        schema::create(&connection)?;
        Ok(Store { connection })
    }

    /// Opens a private in-memory database carrying the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema cannot be created.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// assert!(store.agents().unwrap().is_empty());
    /// ```
    pub fn open_in_memory() -> Result<Store> {
        let connection = Connection::open_in_memory()?;
        schema::create(&connection)?;
        Ok(Store { connection })
    }

    /// Returns every channel in sidebar order.
    ///
    /// # Errors
    ///
    /// Returns an error if a row cannot be read or carries an unknown kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// assert!(store.channels().unwrap().is_empty());
    /// ```
    pub fn channels(&self) -> Result<Vec<Channel>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, group_name, kind, agent_id, unread, sort_index
             FROM channels
             ORDER BY sort_index, id",
        )?;
        let mut rows = statement.query(())?;
        let mut channels = Vec::new();
        while let Some(row) = rows.next()? {
            channels.push(channel_from_row(row)?);
        }
        Ok(channels)
    }

    /// Returns every agent in sidebar order.
    ///
    /// # Errors
    ///
    /// Returns an error if a row cannot be read or carries an unknown status.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// assert!(store.agents().unwrap().is_empty());
    /// ```
    pub fn agents(&self) -> Result<Vec<Agent>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, initials, role, status, status_detail, sort_index
             FROM agents
             ORDER BY sort_index, id",
        )?;
        let mut rows = statement.query(())?;
        let mut agents = Vec::new();
        while let Some(row) = rows.next()? {
            agents.push(agent_from_row(row)?);
        }
        Ok(agents)
    }

    /// Returns the top-level messages of `channel`, oldest first.
    ///
    /// Replies are excluded; they belong to [`Store::thread`]. Messages sharing
    /// a timestamp are ordered by identifier.
    ///
    /// Timestamps are stored as text and sorted lexicographically, so the
    /// ordering only matches the instants the rows carry when every row was
    /// written at `UTC`. Write through [`Store::send`] and [`Store::reply`]
    /// with a `UTC` timestamp; a row carrying any other offset sorts by its
    /// wall clock rather than its instant.
    ///
    /// # Errors
    ///
    /// Returns an error if a row cannot be read or its body cannot be decoded.
    ///
    /// # Examples
    ///
    /// ```
    /// use time::macros::datetime;
    /// use tuclaw_core::model::{ChannelId, Span};
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// let body = [Span::Text("on it".to_string())];
    /// store
    ///     .send(ChannelId(1), &body, datetime!(2026-08-26 09:00 UTC))
    ///     .unwrap();
    /// assert_eq!(store.messages(ChannelId(1)).unwrap().len(), 1);
    /// assert!(store.messages(ChannelId(2)).unwrap().is_empty());
    /// ```
    pub fn messages(&self, channel: ChannelId) -> Result<Vec<Message>> {
        let ChannelId(channel) = channel;
        let mut statement = self.connection.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages
             WHERE channel_id = ?1 AND thread_root_id IS NULL
             ORDER BY sent_at, id"
        ))?;
        let mut rows = statement.query((channel,))?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next()? {
            messages.push(message_from_row(row)?);
        }
        Ok(messages)
    }

    /// Returns the message with the given identifier.
    ///
    /// The thread panel reads its root through this method, because the root
    /// is in neither the selected channel's messages nor its own replies once
    /// the user has switched channels.
    ///
    /// # Errors
    ///
    /// Returns an error if no message carries `id`, or if the row cannot be
    /// read.
    ///
    /// # Examples
    ///
    /// ```
    /// use time::macros::datetime;
    /// use tuclaw_core::model::{ChannelId, Span};
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// let sent = store
    ///     .send(
    ///         ChannelId(1),
    ///         &[Span::Text("on it".to_string())],
    ///         datetime!(2026-08-26 09:00 UTC),
    ///     )
    ///     .unwrap();
    /// assert_eq!(store.message(sent.id).unwrap(), sent);
    /// ```
    pub fn message(&self, id: MessageId) -> Result<Message> {
        let MessageId(id) = id;
        let mut statement = self.connection.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"
        ))?;
        let mut rows = statement.query((id,))?;
        let Some(row) = rows.next()? else {
            bail!("no message with id {id}");
        };
        message_from_row(row)
    }

    /// Returns the replies of the thread rooted at `root`, oldest first.
    ///
    /// The root itself is not part of the result.
    ///
    /// # Errors
    ///
    /// Returns an error if a row cannot be read or its body cannot be decoded.
    ///
    /// # Examples
    ///
    /// ```
    /// use time::macros::datetime;
    /// use tuclaw_core::model::{ChannelId, Span};
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// let root = store
    ///     .send(
    ///         ChannelId(1),
    ///         &[Span::Text("who is in?".to_string())],
    ///         datetime!(2026-08-26 09:00 UTC),
    ///     )
    ///     .unwrap();
    /// assert!(store.thread(root.id).unwrap().is_empty());
    /// ```
    pub fn thread(&self, root: MessageId) -> Result<Vec<Message>> {
        let MessageId(root) = root;
        let mut statement = self.connection.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS}
             FROM messages
             WHERE thread_root_id = ?1
             ORDER BY sent_at, id"
        ))?;
        let mut rows = statement.query((root,))?;
        let mut messages = Vec::new();
        while let Some(row) = rows.next()? {
            messages.push(message_from_row(row)?);
        }
        Ok(messages)
    }

    /// Writes a top-level message from the user and returns the stored row.
    ///
    /// `at` is expected to carry the `UTC` offset, because [`Store::messages`]
    /// orders rows by the stored text.
    ///
    /// # Errors
    ///
    /// Returns an error if the insert fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use time::macros::datetime;
    /// use tuclaw_core::model::{Author, ChannelId, Span};
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// let sent = store
    ///     .send(
    ///         ChannelId(1),
    ///         &[Span::Text("on it".to_string())],
    ///         datetime!(2026-08-26 09:00 UTC),
    ///     )
    ///     .unwrap();
    /// assert_eq!(sent.author, Author::User);
    /// assert_eq!(sent.reply_count, 0);
    /// ```
    pub fn send(&self, channel: ChannelId, body: &[Span], at: OffsetDateTime) -> Result<Message> {
        let ChannelId(channel) = channel;
        let json = encode(body);
        self.connection.execute(
            "INSERT INTO messages
                 (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
             VALUES (?1, NULL, 'user', NULL, ?2, ?3, 0)",
            (channel, &json, at),
        )?;
        Ok(Message {
            id: MessageId(self.connection.last_insert_rowid()),
            author: Author::User,
            body: body.to_vec(),
            sent_at: at,
            reply_count: 0,
        })
    }

    /// Writes a reply from the user into the thread rooted at `root`.
    ///
    /// The insert and the root's `reply_count` increment share one
    /// transaction, so a reply is never stored without the count the feed
    /// reads to draw its affordance. `at` is expected to carry the `UTC`
    /// offset, because [`Store::thread`] orders rows by the stored text.
    ///
    /// # Errors
    ///
    /// Returns an error if no message carries `root`, or if either statement
    /// fails; nothing is written in that case.
    ///
    /// # Examples
    ///
    /// ```
    /// use time::macros::datetime;
    /// use tuclaw_core::model::{ChannelId, Span};
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// let root = store
    ///     .send(
    ///         ChannelId(1),
    ///         &[Span::Text("who is in?".to_string())],
    ///         datetime!(2026-08-26 09:00 UTC),
    ///     )
    ///     .unwrap();
    /// store
    ///     .reply(
    ///         root.id,
    ///         ChannelId(1),
    ///         &[Span::Text("me".to_string())],
    ///         datetime!(2026-08-26 09:05 UTC),
    ///     )
    ///     .unwrap();
    /// assert_eq!(store.thread(root.id).unwrap().len(), 1);
    /// assert_eq!(store.message(root.id).unwrap().reply_count, 1);
    /// ```
    pub fn reply(
        &self,
        root: MessageId,
        channel: ChannelId,
        body: &[Span],
        at: OffsetDateTime,
    ) -> Result<Message> {
        let MessageId(root) = root;
        let ChannelId(channel) = channel;
        let json = encode(body);
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO messages
                 (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
             VALUES (?1, ?2, 'user', NULL, ?3, ?4, 0)",
            (channel, root, &json, at),
        )?;
        let id = transaction.last_insert_rowid();
        let updated = transaction.execute(
            "UPDATE messages SET reply_count = reply_count + 1 WHERE id = ?1",
            (root,),
        )?;
        if updated == 0 {
            bail!("no message with id {root} to reply to");
        }
        transaction.commit()?;
        Ok(Message {
            id: MessageId(id),
            author: Author::User,
            body: body.to_vec(),
            sent_at: at,
            reply_count: 0,
        })
    }

    /// Fills an empty database with the fixture workspace, once.
    ///
    /// The seed marker is written in the same transaction as the fixtures, so
    /// an interrupted first run leaves either everything or nothing. A database
    /// that already carries the marker is left untouched, even when it holds no
    /// rows.
    ///
    /// Every fixture timestamp derives from `now`, so the newest messages fall
    /// on the day the workspace is first opened.
    ///
    /// # Errors
    ///
    /// Returns an error if the marker cannot be read or any insert fails;
    /// nothing is written in that case.
    ///
    /// # Examples
    ///
    /// ```
    /// use time::macros::datetime;
    /// use tuclaw_core::store::Store;
    ///
    /// let store = Store::open_in_memory().unwrap();
    /// store.seed_if_needed(datetime!(2026-08-26 21:00 UTC)).unwrap();
    /// assert_eq!(store.channels().unwrap().len(), 10);
    /// store.seed_if_needed(datetime!(2026-08-27 21:00 UTC)).unwrap();
    /// assert_eq!(store.channels().unwrap().len(), 10);
    /// ```
    pub fn seed_if_needed(&self, now: OffsetDateTime) -> Result<()> {
        let marked: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM seed_marker", (), |row| row.get(0))?;
        if marked > 0 {
            return Ok(());
        }
        let Fixtures {
            agents,
            channels,
            conversations,
        } = fixtures(now);
        let transaction = self.connection.unchecked_transaction()?;
        for Agent {
            id,
            name,
            initials,
            role,
            status,
            sort_index,
        } in agents
        {
            let AgentId(id) = id;
            let (status, detail) = match status {
                AgentStatus::Idle => ("idle", None),
                AgentStatus::Busy(detail) => ("busy", Some(detail)),
            };
            transaction.execute(
                "INSERT INTO agents (id, name, initials, role, status, status_detail, sort_index)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (id, name, initials, role, status, detail, sort_index),
            )?;
        }
        for Channel {
            id,
            name,
            group,
            kind,
            unread,
            sort_index,
        } in channels
        {
            let ChannelId(id) = id;
            let (kind, agent_id) = match kind {
                ChannelKind::Channel => ("channel", None),
                ChannelKind::Direct(AgentId(agent)) => ("direct", Some(agent)),
            };
            transaction.execute(
                "INSERT INTO channels (id, name, group_name, kind, agent_id, unread, sort_index)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                (
                    id,
                    name,
                    group,
                    kind,
                    agent_id,
                    i64::try_from(unread)?,
                    sort_index,
                ),
            )?;
        }
        for Conversation { channel, messages } in conversations {
            let ChannelId(channel) = channel;
            for SeedMessage {
                author,
                body,
                sent_at,
                replies,
            } in messages
            {
                let (author, agent_id) = author_columns(author);
                transaction.execute(
                    "INSERT INTO messages
                         (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
                     VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6)",
                    (
                        channel,
                        author,
                        agent_id,
                        encode(&body),
                        sent_at,
                        i64::try_from(replies.len())?,
                    ),
                )?;
                let root = transaction.last_insert_rowid();
                for SeedReply {
                    author,
                    body,
                    sent_at,
                } in replies
                {
                    let (author, agent_id) = author_columns(author);
                    transaction.execute(
                        "INSERT INTO messages
                             (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                        (channel, root, author, agent_id, encode(&body), sent_at),
                    )?;
                }
            }
        }
        transaction.execute(
            "INSERT INTO seed_marker (id, seeded_at) VALUES (1, ?1)",
            (now,),
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn author_columns(author: Author) -> (&'static str, Option<i64>) {
    match author {
        Author::User => ("user", None),
        Author::Agent(AgentId(id)) => ("agent", Some(id)),
    }
}

fn channel_from_row(row: &Row) -> Result<Channel> {
    let id: i64 = row.get("id")?;
    let name: String = row.get("name")?;
    let group: Option<String> = row.get("group_name")?;
    let kind: String = row.get("kind")?;
    let agent_id: Option<i64> = row.get("agent_id")?;
    let unread: i64 = row.get("unread")?;
    let sort_index: i64 = row.get("sort_index")?;
    let kind = match kind.as_str() {
        "channel" => ChannelKind::Channel,
        "direct" => {
            let Some(agent_id) = agent_id else {
                bail!("channel {id} is direct but names no agent");
            };
            ChannelKind::Direct(AgentId(agent_id))
        }
        kind => bail!("channel {id} carries unknown kind {kind}"),
    };
    Ok(Channel {
        id: ChannelId(id),
        name,
        group,
        kind,
        unread: usize::try_from(unread)?,
        sort_index,
    })
}

fn agent_from_row(row: &Row) -> Result<Agent> {
    let id: i64 = row.get("id")?;
    let name: String = row.get("name")?;
    let initials: String = row.get("initials")?;
    let role: String = row.get("role")?;
    let status: String = row.get("status")?;
    let detail: Option<String> = row.get("status_detail")?;
    let sort_index: i64 = row.get("sort_index")?;
    let status = match status.as_str() {
        "idle" => AgentStatus::Idle,
        "busy" => {
            let Some(detail) = detail else {
                bail!("agent {id} is busy but names no task");
            };
            AgentStatus::Busy(detail)
        }
        status => bail!("agent {id} carries unknown status {status}"),
    };
    Ok(Agent {
        id: AgentId(id),
        name,
        initials,
        role,
        status,
        sort_index,
    })
}

fn message_from_row(row: &Row) -> Result<Message> {
    let id: i64 = row.get("id")?;
    let author: String = row.get("author")?;
    let agent_id: Option<i64> = row.get("agent_id")?;
    let body: String = row.get("body")?;
    let sent_at: OffsetDateTime = row.get("sent_at")?;
    let reply_count: i64 = row.get("reply_count")?;
    let author = match author.as_str() {
        "user" => Author::User,
        "agent" => {
            let Some(agent_id) = agent_id else {
                bail!("message {id} is from an agent but names none");
            };
            Author::Agent(AgentId(agent_id))
        }
        author => bail!("message {id} carries unknown author {author}"),
    };
    Ok(Message {
        id: MessageId(id),
        author,
        body: decode(&body)?,
        sent_at,
        reply_count: usize::try_from(reply_count)?,
    })
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;
    use time::macros::datetime;

    use super::Store;
    use crate::model::{
        Agent, AgentId, AgentStatus, Author, Channel, ChannelId, ChannelKind, MessageId, Span,
    };

    fn text(body: &str) -> Vec<Span> {
        vec![Span::Text(body.to_string())]
    }

    fn ids(messages: &[crate::model::Message]) -> Vec<i64> {
        let mut ids = Vec::new();
        for message in messages {
            let MessageId(raw) = message.id;
            ids.push(raw);
        }
        ids
    }

    fn bodies(messages: &[crate::model::Message]) -> Vec<Vec<Span>> {
        let mut bodies = Vec::new();
        for message in messages {
            bodies.push(message.body.clone());
        }
        bodies
    }

    #[test]
    fn an_in_memory_store_starts_empty() {
        let store = Store::open_in_memory().expect("the schema is created");
        assert!(store.channels().expect("channels read").is_empty());
        assert!(store.agents().expect("agents read").is_empty());
        assert!(
            store
                .messages(ChannelId(1))
                .expect("messages read")
                .is_empty()
        );
    }

    #[test]
    fn a_message_is_read_back_in_its_channel_only() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .send(
                ChannelId(1),
                &text("on it"),
                datetime!(2026-08-26 09:00 UTC),
            )
            .expect("the message is written");
        let messages = store.messages(ChannelId(1)).expect("messages read");
        assert_eq!(bodies(&messages), vec![text("on it")]);
        assert!(
            store
                .messages(ChannelId(2))
                .expect("messages read")
                .is_empty()
        );
    }

    #[test]
    fn message_returns_exactly_the_row_send_returned() {
        let store = Store::open_in_memory().expect("the schema is created");
        let sent = store
            .send(
                ChannelId(1),
                &text("watching it tonight"),
                datetime!(2026-08-26 09:00 UTC),
            )
            .expect("the message is written");
        let read = store.message(sent.id).expect("the message is read");
        assert_eq!(read, sent);
    }

    #[test]
    fn message_for_a_missing_id_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        assert!(store.message(MessageId(404)).is_err());
    }

    #[test]
    fn a_reply_lands_in_the_thread_and_raises_the_root_count() {
        let store = Store::open_in_memory().expect("the schema is created");
        let root = store
            .send(
                ChannelId(1),
                &text("who is in?"),
                datetime!(2026-08-26 09:00 UTC),
            )
            .expect("the root is written");
        store
            .reply(
                root.id,
                ChannelId(1),
                &text("me"),
                datetime!(2026-08-26 09:05 UTC),
            )
            .expect("the reply is written");
        let feed = store.messages(ChannelId(1)).expect("messages read");
        assert_eq!(ids(&feed), ids(std::slice::from_ref(&root)));
        assert_eq!(feed[0].reply_count, 1);
        let replies = store.thread(root.id).expect("the thread is read");
        assert_eq!(bodies(&replies), vec![text("me")]);
        assert_eq!(
            store
                .message(root.id)
                .expect("the root is read")
                .reply_count,
            1
        );
    }

    #[test]
    fn a_thread_without_replies_is_empty() {
        let store = Store::open_in_memory().expect("the schema is created");
        let root = store
            .send(
                ChannelId(1),
                &text("who is in?"),
                datetime!(2026-08-26 09:00 UTC),
            )
            .expect("the root is written");
        assert!(
            store
                .thread(root.id)
                .expect("the thread is read")
                .is_empty()
        );
    }

    #[test]
    fn replying_to_a_missing_root_writes_nothing() {
        let store = Store::open_in_memory().expect("the schema is created");
        let failed = store.reply(
            MessageId(404),
            ChannelId(1),
            &text("me"),
            datetime!(2026-08-26 09:05 UTC),
        );
        assert!(failed.is_err());
        assert!(
            store
                .thread(MessageId(404))
                .expect("the thread is read")
                .is_empty()
        );
    }

    #[test]
    fn messages_sharing_a_timestamp_are_ordered_by_id() {
        let store = Store::open_in_memory().expect("the schema is created");
        let at = datetime!(2026-08-26 09:00 UTC);
        let first = store
            .send(ChannelId(1), &text("first"), at)
            .expect("the message is written");
        let second = store
            .send(ChannelId(1), &text("second"), at)
            .expect("the message is written");
        let later = store
            .send(
                ChannelId(1),
                &text("later"),
                datetime!(2026-08-26 09:01 UTC),
            )
            .expect("the message is written");
        let earlier = store
            .send(
                ChannelId(1),
                &text("earlier"),
                datetime!(2026-08-26 08:00 UTC),
            )
            .expect("the message is written");
        let messages = store.messages(ChannelId(1)).expect("messages read");
        assert_eq!(ids(&messages), ids(&[earlier, first, second, later]));
    }

    #[test]
    fn a_body_with_spans_round_trips_through_the_database() {
        let store = Store::open_in_memory().expect("the schema is created");
        let body = vec![
            Span::Text("привет ".to_string()),
            Span::Mention("allspeak".to_string()),
            Span::Text(", drop it in ".to_string()),
            Span::Code("~/Media/inbox".to_string()),
        ];
        store
            .send(ChannelId(1), &body, datetime!(2026-08-26 09:00 UTC))
            .expect("the message is written");
        let messages = store.messages(ChannelId(1)).expect("messages read");
        assert_eq!(messages[0].body, body);
    }

    #[test]
    fn a_non_utc_timestamp_reads_back_equal() {
        let store = Store::open_in_memory().expect("the schema is created");
        let at = datetime!(2026-08-26 09:00:30 +03:00);
        let sent = store
            .send(ChannelId(1), &text("on it"), at)
            .expect("the message is written");
        let read = store.message(sent.id).expect("the message is read");
        assert_eq!(read.sent_at, at);
        assert_eq!(
            read.sent_at.to_offset(time::macros::offset!(UTC)),
            at.to_offset(time::macros::offset!(UTC))
        );
        assert_eq!(read.sent_at.unix_timestamp(), at.unix_timestamp());
        assert_eq!(read.sent_at.offset(), at.offset());
    }

    #[test]
    fn channels_and_agents_map_back_to_domain_types() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .connection
            .execute(
                "INSERT INTO agents (id, name, initials, role, status, status_detail, sort_index)
                 VALUES (1, 'allspeak', 'as', 'translates', 'busy', 'subtitling', 0),
                        (2, 'media review', 'mr', 'watches', 'idle', NULL, 1)",
                (),
            )
            .expect("the agents are written");
        store
            .connection
            .execute(
                "INSERT INTO channels (id, name, group_name, kind, agent_id, unread, sort_index)
                 VALUES (10, 'movie-night', 'Movie nights', 'channel', NULL, 0, 1),
                        (11, 'allspeak', NULL, 'direct', 1, 2, 0)",
                (),
            )
            .expect("the channels are written");
        let agents = store.agents().expect("agents read");
        assert_eq!(
            agents,
            vec![
                Agent {
                    id: AgentId(1),
                    name: "allspeak".to_string(),
                    initials: "as".to_string(),
                    role: "translates".to_string(),
                    status: AgentStatus::Busy("subtitling".to_string()),
                    sort_index: 0,
                },
                Agent {
                    id: AgentId(2),
                    name: "media review".to_string(),
                    initials: "mr".to_string(),
                    role: "watches".to_string(),
                    status: AgentStatus::Idle,
                    sort_index: 1,
                },
            ]
        );
        let channels = store.channels().expect("channels read");
        assert_eq!(
            channels,
            vec![
                Channel {
                    id: ChannelId(11),
                    name: "allspeak".to_string(),
                    group: None,
                    kind: ChannelKind::Direct(AgentId(1)),
                    unread: 2,
                    sort_index: 0,
                },
                Channel {
                    id: ChannelId(10),
                    name: "movie-night".to_string(),
                    group: Some("Movie nights".to_string()),
                    kind: ChannelKind::Channel,
                    unread: 0,
                    sort_index: 1,
                },
            ]
        );
    }

    #[test]
    fn an_agent_authored_message_reads_back_as_that_agent() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .connection
            .execute(
                "INSERT INTO messages
                     (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
                 VALUES (1, NULL, 'agent', 7, '[{\"Text\":\"on it\"}]', ?1, 0)",
                (datetime!(2026-08-26 09:00 UTC),),
            )
            .expect("the message is written");
        let messages = store.messages(ChannelId(1)).expect("messages read");
        assert_eq!(messages[0].author, Author::Agent(AgentId(7)));
    }

    #[test]
    fn a_row_with_an_unknown_author_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        let at: OffsetDateTime = datetime!(2026-08-26 09:00 UTC);
        store
            .connection
            .execute(
                "INSERT INTO messages
                     (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
                 VALUES (1, NULL, 'ghost', NULL, '[]', ?1, 0)",
                (at,),
            )
            .expect("the message is written");
        assert!(store.messages(ChannelId(1)).is_err());
    }

    #[test]
    fn a_message_from_an_agent_that_names_none_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        let at: OffsetDateTime = datetime!(2026-08-26 09:00 UTC);
        store
            .connection
            .execute(
                "INSERT INTO messages
                     (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
                 VALUES (1, NULL, 'agent', NULL, '[]', ?1, 0)",
                (at,),
            )
            .expect("the message is written");
        assert!(store.messages(ChannelId(1)).is_err());
    }

    #[test]
    fn a_message_carrying_an_undecodable_body_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        let at: OffsetDateTime = datetime!(2026-08-26 09:00 UTC);
        store
            .connection
            .execute(
                "INSERT INTO messages
                     (channel_id, thread_root_id, author, agent_id, body, sent_at, reply_count)
                 VALUES (1, NULL, 'user', NULL, 'not json', ?1, 0)",
                (at,),
            )
            .expect("the message is written");
        assert!(store.messages(ChannelId(1)).is_err());
    }

    #[test]
    fn a_direct_channel_that_names_no_agent_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .connection
            .execute(
                "INSERT INTO channels (id, name, group_name, kind, agent_id, unread, sort_index)
                 VALUES (1, 'allspeak', NULL, 'direct', NULL, 0, 0)",
                (),
            )
            .expect("the channel is written");
        assert!(store.channels().is_err());
    }

    #[test]
    fn a_channel_carrying_an_unknown_kind_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .connection
            .execute(
                "INSERT INTO channels (id, name, group_name, kind, agent_id, unread, sort_index)
                 VALUES (1, 'movie-night', NULL, 'broadcast', NULL, 0, 0)",
                (),
            )
            .expect("the channel is written");
        assert!(store.channels().is_err());
    }

    #[test]
    fn a_busy_agent_that_names_no_task_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .connection
            .execute(
                "INSERT INTO agents (id, name, initials, role, status, status_detail, sort_index)
                 VALUES (1, 'allspeak', 'AL', 'translator', 'busy', NULL, 0)",
                (),
            )
            .expect("the agent is written");
        assert!(store.agents().is_err());
    }

    #[test]
    fn an_agent_carrying_an_unknown_status_is_an_error() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .connection
            .execute(
                "INSERT INTO agents (id, name, initials, role, status, status_detail, sort_index)
                 VALUES (1, 'allspeak', 'AL', 'translator', 'asleep', NULL, 0)",
                (),
            )
            .expect("the agent is written");
        assert!(store.agents().is_err());
    }

    fn seeded() -> Store {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("the fixtures are written");
        store
    }

    fn channel_named(store: &Store, name: &str) -> ChannelId {
        let channels = store.channels().expect("channels read");
        let mut found = None;
        for channel in channels {
            if channel.name == name {
                found = Some(channel.id);
                break;
            }
        }
        found.expect("the fixtures carry that channel")
    }

    #[test]
    fn seeding_writes_the_agents_and_channels_in_order() {
        let store = seeded();
        let mut agents = Vec::new();
        for agent in store.agents().expect("agents read") {
            agents.push(agent.name);
        }
        assert_eq!(
            agents,
            vec![
                "magnet feed sync".to_string(),
                "allspeak".to_string(),
                "media review".to_string(),
                "tuclaw general".to_string(),
            ]
        );
        let channels = store.channels().expect("channels read");
        assert_eq!(channels.len(), 10);
        assert_eq!(channels[0].name, "movie-night");
        assert_eq!(channels[9].name, "media review");
        assert_eq!(channels[9].kind, ChannelKind::Direct(AgentId(3)));
    }

    #[test]
    fn seeding_gives_movie_night_fifty_eight_top_level_messages() {
        let store = seeded();
        let channel = channel_named(&store, "movie-night");
        let messages = store.messages(channel).expect("messages read");
        assert_eq!(messages.len(), 58);
        let mut agent_authored = 0;
        for message in &messages {
            match message.author {
                Author::User => {}
                Author::Agent(_) => agent_authored += 1,
            }
        }
        assert!(agent_authored > 0);
    }

    #[test]
    fn the_seeded_thread_root_carries_four_replies() {
        let store = seeded();
        let channel = channel_named(&store, "movie-night");
        let messages = store.messages(channel).expect("messages read");
        let mut roots = Vec::new();
        for message in &messages {
            if message.reply_count > 0 {
                roots.push(message.id);
            }
        }
        assert_eq!(roots.len(), 1);
        let root = store.message(roots[0]).expect("the root is read");
        assert_eq!(root.reply_count, 4);
        assert_eq!(store.thread(root.id).expect("the thread is read").len(), 4);
    }

    #[test]
    fn personal_is_the_only_seeded_channel_without_messages() {
        let store = seeded();
        let mut empty = Vec::new();
        for channel in store.channels().expect("channels read") {
            if store
                .messages(channel.id)
                .expect("messages read")
                .is_empty()
            {
                empty.push(channel.name);
            }
        }
        assert_eq!(empty, vec!["personal".to_string()]);
    }

    #[test]
    fn seeded_bodies_keep_their_mentions_and_code() {
        let store = seeded();
        let channel = channel_named(&store, "movie-night");
        let mut mentions = 0;
        let mut codes = 0;
        for message in store.messages(channel).expect("messages read") {
            for span in message.body {
                match span {
                    Span::Text(_) => {}
                    Span::Mention(_) => mentions += 1,
                    Span::Code(_) => codes += 1,
                }
            }
        }
        assert!(mentions > 0);
        assert!(codes > 0);
    }

    #[test]
    fn the_seeded_days_cover_today_and_yesterday() {
        let store = seeded();
        let now = datetime!(2026-08-26 21:00 UTC);
        let channel = channel_named(&store, "movie-night");
        let sections = crate::grouping::group_by_day(
            &store.messages(channel).expect("messages read"),
            time::macros::offset!(UTC),
            now,
        );
        let mut titles = Vec::new();
        for section in &sections {
            titles.push(section.title.clone());
        }
        assert_eq!(sections.len(), 15);
        assert_eq!(titles[13], "Yesterday".to_string());
        assert_eq!(titles[14], "Today".to_string());
    }

    #[test]
    fn seeding_twice_leaves_one_copy() {
        let store = seeded();
        store
            .seed_if_needed(datetime!(2026-08-27 21:00 UTC))
            .expect("the second seed is a no-op");
        assert_eq!(store.channels().expect("channels read").len(), 10);
        assert_eq!(store.agents().expect("agents read").len(), 4);
        let channel = channel_named(&store, "movie-night");
        assert_eq!(store.messages(channel).expect("messages read").len(), 58);
    }

    #[test]
    fn a_database_carrying_a_marker_but_no_rows_is_left_alone() {
        let store = Store::open_in_memory().expect("the schema is created");
        store
            .connection
            .execute(
                "INSERT INTO seed_marker (id, seeded_at) VALUES (1, ?1)",
                (datetime!(2026-08-26 21:00 UTC),),
            )
            .expect("the marker is written");
        store
            .seed_if_needed(datetime!(2026-08-26 21:00 UTC))
            .expect("seeding is skipped");
        assert!(store.channels().expect("channels read").is_empty());
        assert!(store.agents().expect("agents read").is_empty());
    }

    #[test]
    fn seeding_a_reopened_file_database_writes_nothing_twice() {
        let path =
            std::env::temp_dir().join(format!("tuclaw-seed-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let now = datetime!(2026-08-26 21:00 UTC);
        let store = Store::open(&path).expect("the database is created");
        store.seed_if_needed(now).expect("the fixtures are written");
        drop(store);
        let store = Store::open(&path).expect("the database is reopened");
        store
            .seed_if_needed(now)
            .expect("the second seed is a no-op");
        assert_eq!(store.channels().expect("channels read").len(), 10);
        let channel = channel_named(&store, "movie-night");
        assert_eq!(store.messages(channel).expect("messages read").len(), 58);
        drop(store);
        std::fs::remove_file(&path).expect("the test database is removed");
    }

    #[test]
    fn reopening_a_file_database_keeps_its_messages() {
        let path =
            std::env::temp_dir().join(format!("tuclaw-store-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path).expect("the database is created");
        store
            .send(
                ChannelId(1),
                &text("on it"),
                datetime!(2026-08-26 09:00 UTC),
            )
            .expect("the message is written");
        drop(store);
        let store = Store::open(&path).expect("the database is reopened");
        assert_eq!(
            bodies(&store.messages(ChannelId(1)).expect("messages read")).len(),
            1
        );
        drop(store);
        std::fs::remove_file(&path).expect("the test database is removed");
    }
}
