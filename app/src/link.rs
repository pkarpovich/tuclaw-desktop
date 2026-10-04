use tuclaw_core::model::{
    Agent, AgentId, AgentStatus, Author, Channel, ChannelId, ChannelKind, Message, MessageId,
    RecordingId, Span, Voice,
};
use tuclaw_core::v3;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

const VOICE_HEADER: &str = "[Voice message";

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
    let voice = voice(&message.attachments);
    let text = match voice {
        Some(_) => transcript(&message.text),
        None => message.text.as_str(),
    };
    Message {
        id: MessageId(id),
        author,
        body: vec![Span::Text(text.to_string())],
        sent_at: message.created_at,
        voice,
    }
}

fn voice(attachments: &[v3::Attachment]) -> Option<Voice> {
    for attachment in attachments {
        let v3::Attachment {
            id: v3::AttachmentId(id),
            kind,
            mime,
            size_bytes: _,
            duration_ms,
        } = attachment;
        match kind {
            v3::AttachmentKind::Voice => {
                return Some(Voice {
                    recording: RecordingId(*id),
                    mime: mime.clone(),
                    duration: duration_ms.map(Duration::from_millis),
                });
            }
            v3::AttachmentKind::Unknown => {}
        }
    }
    None
}

fn transcript(text: &str) -> &str {
    let Some((header, rest)) = text.split_once('\n') else {
        return text;
    };
    let header = header.trim();
    if header.starts_with(VOICE_HEADER) && header.ends_with(']') {
        rest.trim_start()
    } else {
        text
    }
}

pub fn recording_id(recording: RecordingId) -> v3::AttachmentId {
    let RecordingId(raw) = recording;
    v3::AttachmentId(raw)
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
    Snapshot,
    Daemon(String),
}

pub struct Config {
    pub daemon_url: Option<String>,
    pub token: Option<String>,
    pub world: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Config {
        let world = match std::env::var("TUCLAW_MOCK_WORLD") {
            Ok(path) => Some(PathBuf::from(path)),
            Err(_) => default_world(),
        };
        Config {
            daemon_url: std::env::var("TUCLAW_DAEMON_URL").ok(),
            token: std::env::var("TUCLAW_CLIENT_TOKEN").ok(),
            world,
        }
    }

    pub fn client(self) -> Result<(v3::Client, Source), String> {
        let Config {
            daemon_url,
            token,
            world,
        } = self;
        let Some(url) = daemon_url else {
            let Some(world) = world else {
                let mock = v3::MockTransport::new(v3::Scenario::default(), v3::Pace::Realtime);
                return Ok((v3::Client::mock(&mock), Source::Mock));
            };
            let seed = load_seed(&world)?;
            let mock = v3::MockTransport::seeded(seed, v3::Scenario::default(), v3::Pace::Realtime);
            return Ok((v3::Client::mock(&mock), Source::Snapshot));
        };
        let token = token.map(v3::ClientToken);
        let client = v3::Client::http(&url, token).map_err(|error| error.to_string())?;
        Ok((client, Source::Daemon(url)))
    }
}

pub fn peak_directory(source: &Source) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("tuclaw-desktop")
            .join("waveforms")
            .join(source_key(source)),
    )
}

fn source_key(source: &Source) -> String {
    let url = match source {
        Source::Mock => return "mock".to_string(),
        Source::Snapshot => return "snapshot".to_string(),
        Source::Daemon(url) => url,
    };
    let mut key = String::new();
    for letter in url.trim_start_matches("http://").chars() {
        if letter.is_ascii_alphanumeric() {
            key.push(letter);
        } else {
            key.push('-');
        }
    }
    key
}

fn default_world() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("tuclaw-desktop")
        .join("world.json");
    if path.is_file() { Some(path) } else { None }
}

pub fn load_seed(path: &Path) -> Result<v3::Seed, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_reader(BufReader::new(file))
        .map_err(|error| format!("{}: {error}", path.display()))
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
            attachments: Vec::new(),
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
        assert_eq!(mapped.voice, None);
    }

    fn spoken(text: &str, attachments: &str) -> v3::Message {
        serde_json::from_str(&format!(
            r#"{{"id": 9, "surface_id": 1, "kind": "user", "author": {{"kind": "user"}},
                "text": {text:?}, "created_at": "2026-10-03T15:26:00Z", "attachments": {attachments}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_voice_attachment_becomes_the_recording_and_drops_the_header() {
        let mapped = message(&spoken(
            "[Voice message]\nПоставь кроваво-красный везде.",
            r#"[{"id": 8, "kind": "voice", "mime": "audio/mp4", "size_bytes": 31257, "duration_ms": 3920}]"#,
        ));
        assert_eq!(
            mapped.voice,
            Some(Voice {
                recording: RecordingId(8),
                mime: "audio/mp4".into(),
                duration: Some(Duration::from_millis(3920)),
            })
        );
        assert_eq!(
            mapped.body,
            vec![Span::Text("Поставь кроваво-красный везде.".into())]
        );
        let forwarded = message(&spoken(
            "[Voice message from Vlad, 2026-10-03]\nПривет",
            r#"[{"id": 2, "kind": "voice"}]"#,
        ));
        assert_eq!(forwarded.body, vec![Span::Text("Привет".into())]);
        assert_eq!(forwarded.voice.map(|voice| voice.duration), Some(None));
    }

    #[test]
    fn text_without_a_voice_attachment_keeps_its_header() {
        let plain = message(&spoken("[Voice message]\nтекст", "[]"));
        assert_eq!(plain.voice, None);
        assert_eq!(
            plain.body,
            vec![Span::Text("[Voice message]\nтекст".into())]
        );
        let other = message(&spoken(
            "[Voice message]\nтекст",
            r#"[{"id": 3, "kind": "photo"}]"#,
        ));
        assert_eq!(other.voice, None);
        let unheaded = message(&spoken("just words", r#"[{"id": 4, "kind": "voice"}]"#));
        assert_eq!(unheaded.body, vec![Span::Text("just words".into())]);
    }

    #[test]
    fn every_source_keeps_its_own_waveforms() {
        assert_eq!(source_key(&Source::Mock), "mock");
        assert_eq!(source_key(&Source::Snapshot), "snapshot");
        assert_eq!(
            source_key(&Source::Daemon("http://192.168.1.10:9090".into())),
            "192-168-1-10-9090"
        );
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
            world: None,
        };
        let Ok((_client, Source::Mock)) = mock.client() else {
            panic!("no daemon URL means the mock");
        };
        let daemon = Config {
            daemon_url: Some("http://192.168.1.10:9090".into()),
            token: Some("t".into()),
            world: None,
        };
        let Ok((_client, Source::Daemon(url))) = daemon.client() else {
            panic!("a URL and a token mean the daemon");
        };
        assert_eq!(url, "http://192.168.1.10:9090");
        let tokenless = Config {
            daemon_url: Some("http://host:9090".into()),
            token: None,
            world: None,
        };
        let Ok((_client, Source::Daemon(_))) = tokenless.client() else {
            panic!("a URL without a token is an open daemon");
        };
        let bad = Config {
            daemon_url: Some("ftp://host".into()),
            token: Some("t".into()),
            world: None,
        };
        assert!(bad.client().is_err());
    }

    #[test]
    fn a_world_file_seeds_the_mock_and_a_bad_one_fails() {
        let directory = std::env::temp_dir().join(format!("tuclaw-world-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("the temporary directory is created");
        let world = directory.join("world.json");
        let surfaces = include_str!("../../core/testdata/v3/surfaces.json");
        let agents = include_str!("../../core/testdata/v3/agents.json");
        std::fs::write(
            &world,
            format!(r#"{{"surfaces": {surfaces}, "agents": {agents}}}"#),
        )
        .expect("the world is written");
        let seeded = Config {
            daemon_url: None,
            token: None,
            world: Some(world.clone()),
        };
        let Ok((_client, Source::Snapshot)) = seeded.client() else {
            panic!("a world file means the snapshot");
        };
        std::fs::write(&world, "not json").expect("the world is overwritten");
        let broken = Config {
            daemon_url: None,
            token: None,
            world: Some(world.clone()),
        };
        let Err(error) = broken.client() else {
            panic!("a broken world fails to start");
        };
        assert!(error.contains("world.json"), "{error}");
        std::fs::remove_dir_all(&directory).expect("the temporary directory is removed");
    }
}
