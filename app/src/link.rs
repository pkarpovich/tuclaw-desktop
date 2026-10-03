use tuclaw_core::model::{
    Agent, AgentId, AgentStatus, Author, Channel, ChannelId, ChannelKind, Message, MessageId, Span,
};
use tuclaw_core::v3;

pub fn channel(surface: &v3::Surface) -> Channel {
    let v3::SurfaceId(id) = surface.id;
    Channel {
        id: ChannelId(id),
        name: surface.name.clone(),
        group: None,
        kind: ChannelKind::Channel,
        unread: 0,
        sort_index: surface.sort_order,
    }
}

pub fn surface_id(channel: ChannelId) -> v3::SurfaceId {
    let ChannelId(id) = channel;
    v3::SurfaceId(id)
}

pub fn agent_id(agent: v3::AgentId) -> AgentId {
    let v3::AgentId(id) = agent;
    AgentId(id)
}

pub fn agent(agent: &v3::Agent, busy_on: Option<&str>) -> Agent {
    let v3::Agent {
        id,
        name,
        ident,
        description,
        bot_username: _,
        model,
        state: _,
        live_run: _,
        home_surface_id: _,
    } = agent;
    let status = match busy_on {
        Some(surface) => AgentStatus::Busy(format!("in #{surface}")),
        None => AgentStatus::Idle,
    };
    let mut role = format!("@{ident}");
    if !description.is_empty() {
        role = format!("{description} · {role}");
    }
    if !model.is_empty() {
        role = format!("{role} · {model}");
    }
    let v3::AgentId(raw) = *id;
    Agent {
        id: AgentId(raw),
        name: name.clone(),
        initials: initials(name),
        role,
        status,
        sort_index: raw,
    }
}

pub fn message(message: &v3::Message) -> Message {
    let v3::MessageId(id) = message.id;
    let author = match (message.author.kind, message.author.agent_id) {
        (v3::AuthorKind::User, _) => Author::User,
        (v3::AuthorKind::Agent, Some(agent)) => Author::Agent(agent_id(agent)),
        (v3::AuthorKind::Agent, None) => Author::System,
        (v3::AuthorKind::System, _) => Author::System,
        (v3::AuthorKind::Unknown, _) => Author::System,
    };
    Message {
        id: MessageId(id),
        author,
        body: vec![Span::Text(message.text.clone())],
        sent_at: message.created_at,
    }
}

pub fn initials(name: &str) -> String {
    let mut words = Vec::new();
    for word in name.split_whitespace() {
        words.push(word);
    }
    let mut initials = String::new();
    match words.as_slice() {
        [] => initials.push_str("··"),
        [single] => {
            for letter in single.chars().take(2) {
                initials.extend(letter.to_uppercase());
            }
        }
        [first, second, ..] => {
            for word in [first, second] {
                if let Some(letter) = word.chars().next() {
                    initials.extend(letter.to_uppercase());
                }
            }
        }
    }
    initials
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Mock,
    Daemon(String),
}

pub struct Config {
    pub daemon_url: Option<String>,
    pub token: Option<String>,
}

impl Config {
    pub fn from_env() -> Config {
        Config {
            daemon_url: std::env::var("TUCLAW_DAEMON_URL").ok(),
            token: std::env::var("TUCLAW_CLIENT_TOKEN").ok(),
        }
    }

    pub fn client(self) -> Result<(v3::Client, Source), String> {
        let Config { daemon_url, token } = self;
        let Some(url) = daemon_url else {
            let mock = v3::MockTransport::new(v3::Scenario::default(), v3::Pace::Realtime);
            return Ok((v3::Client::mock(&mock), Source::Mock));
        };
        let Some(token) = token else {
            return Err("TUCLAW_DAEMON_URL is set but TUCLAW_CLIENT_TOKEN is not".to_string());
        };
        let client =
            v3::Client::http(&url, v3::ClientToken(token)).map_err(|error| error.to_string())?;
        Ok((client, Source::Daemon(url)))
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    fn surface() -> v3::Surface {
        serde_json::from_str(
            r#"{"id": 4, "kind": "channel", "name": "Magnet Feed", "sort_order": 2}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_surface_becomes_an_ungrouped_channel() {
        assert_eq!(
            channel(&surface()),
            Channel {
                id: ChannelId(4),
                name: "Magnet Feed".into(),
                group: None,
                kind: ChannelKind::Channel,
                unread: 0,
                sort_index: 2,
            }
        );
    }

    #[test]
    fn an_agent_is_busy_on_the_surface_of_its_live_run() {
        let raw: v3::Agent = serde_json::from_str(
            r#"{"id": 3, "name": "Magnet Feed", "ident": "magnet_feed", "state": "running"}"#,
        )
        .unwrap();
        let busy = agent(&raw, Some("General"));
        assert_eq!(busy.status, AgentStatus::Busy("in #General".into()));
        assert_eq!(busy.initials, "MF");
        assert_eq!(busy.role, "@magnet_feed");
        let described: v3::Agent = serde_json::from_str(
            r#"{"id": 1, "name": "Jarvis", "ident": "tuclaw", "description": "The house butler", "model": "opus[1m]:medium", "state": "idle"}"#,
        )
        .unwrap();
        assert_eq!(
            agent(&described, None).role,
            "The house butler · @tuclaw · opus[1m]:medium"
        );
        assert_eq!(agent(&raw, None).status, AgentStatus::Idle);
    }

    #[test]
    fn authors_map_by_kind() {
        let at = datetime!(2026-10-03 15:26 UTC);
        let make = |kind: v3::AuthorKind, agent: Option<i64>| v3::Message {
            id: v3::MessageId(1),
            surface_id: v3::SurfaceId(1),
            kind: v3::MessageKind::Answer,
            author: v3::Author {
                kind,
                agent_id: agent.map(v3::AgentId),
            },
            addressed_agent_id: None,
            reply_to_message_id: None,
            text: "hi".into(),
            run_id: None,
            origin: "user".into(),
            channel: None,
            client_message_id: None,
            created_at: at,
            run_summary: None,
        };
        assert_eq!(
            message(&make(v3::AuthorKind::User, None)).author,
            Author::User
        );
        assert_eq!(
            message(&make(v3::AuthorKind::Agent, Some(3))).author,
            Author::Agent(AgentId(3))
        );
        assert_eq!(
            message(&make(v3::AuthorKind::Agent, None)).author,
            Author::System
        );
        assert_eq!(
            message(&make(v3::AuthorKind::System, Some(3))).author,
            Author::System
        );
        let mapped = message(&make(v3::AuthorKind::User, None));
        assert_eq!(mapped.body, vec![Span::Text("hi".into())]);
        assert_eq!(mapped.sent_at, at);
    }

    #[test]
    fn initials_take_two_words_or_two_letters() {
        assert_eq!(initials("Magnet Feed Sync"), "MF");
        assert_eq!(initials("jarvis"), "JA");
        assert_eq!(initials("Я"), "Я");
        assert_eq!(initials(""), "··");
    }

    #[test]
    fn the_source_follows_the_environment() {
        let mock = Config {
            daemon_url: None,
            token: None,
        };
        let Ok((_client, Source::Mock)) = mock.client() else {
            panic!("no daemon URL means the mock");
        };
        let daemon = Config {
            daemon_url: Some("http://192.168.1.10:9090".into()),
            token: Some("t".into()),
        };
        let Ok((_client, Source::Daemon(url))) = daemon.client() else {
            panic!("a URL and a token mean the daemon");
        };
        assert_eq!(url, "http://192.168.1.10:9090");
        let tokenless = Config {
            daemon_url: Some("http://host:9090".into()),
            token: None,
        };
        assert!(tokenless.client().is_err());
        let bad = Config {
            daemon_url: Some("ftp://host".into()),
            token: Some("t".into()),
        };
        assert!(bad.client().is_err());
    }
}
