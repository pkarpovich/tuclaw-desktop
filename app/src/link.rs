use tuclaw_core::model::{
    Agent, AgentId, AgentStatus, Author, Channel, ChannelId, ChannelKind, Message, MessageId,
    Picture, RecordingId, RunOutcome, RunRef, Span, Voice,
};
use tuclaw_core::v3;

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

const VOICE_HEADER: &str = "[Voice message";
const SILENT: &str = "[SILENT]";

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

pub fn v3_agent_id(agent: AgentId) -> v3::AgentId {
    let AgentId(id) = agent;
    v3::AgentId(id)
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
        avatar_url,
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
        picture: picture(avatar_url.as_ref()),
    }
}

pub fn picture(url: Option<&v3::AvatarUrl>) -> Option<Picture> {
    let v3::AvatarUrl(url) = url?;
    Some(Picture(url.clone()))
}

pub fn avatar_url(picture: &Picture) -> v3::AvatarUrl {
    let Picture(url) = picture;
    v3::AvatarUrl(url.clone())
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
    let text = match message.kind {
        v3::MessageKind::Answer if text.trim() == SILENT => "",
        v3::MessageKind::Answer => text,
        v3::MessageKind::User => text,
        v3::MessageKind::Post => text,
        v3::MessageKind::Notice => text,
        v3::MessageKind::A2a => text,
        v3::MessageKind::Unknown => text,
    };
    Message {
        id: MessageId(id),
        author,
        body: vec![Span::Text(text.to_string())],
        sent_at: message.created_at,
        voice,
        run: run_ref(message),
    }
}

fn run_ref(message: &v3::Message) -> Option<RunRef> {
    match message.kind {
        v3::MessageKind::Answer => {}
        v3::MessageKind::User => return None,
        v3::MessageKind::Post => return None,
        v3::MessageKind::Notice => return None,
        v3::MessageKind::A2a => return None,
        v3::MessageKind::Unknown => return None,
    }
    let v3::RunId(id) = message.run_id.as_ref()?;
    let v3::RunSummary {
        status,
        step_count,
        tool_count,
        duration_ms,
    } = message.run_summary?;
    let outcome = match status {
        v3::RunStatus::Running => RunOutcome::Running,
        v3::RunStatus::Ok => RunOutcome::Ok,
        v3::RunStatus::Error => RunOutcome::Error,
        v3::RunStatus::Interrupted => RunOutcome::Interrupted,
        v3::RunStatus::Unknown => RunOutcome::Unknown,
    };
    Some(RunRef {
        id: id.clone(),
        outcome,
        steps: step_count,
        tools: tool_count,
        duration: Duration::from_millis(duration_ms),
    })
}

pub fn run_id(run: &RunRef) -> v3::RunId {
    v3::RunId(run.id.clone())
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
    let trimmed = text.trim_start();
    if !trimmed.starts_with(VOICE_HEADER) {
        return text;
    }
    let Some((header, rest)) = trimmed.split_once(']') else {
        return text;
    };
    if header.contains('\n') {
        return text;
    }
    rest.trim_start()
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

pub const DEFAULT_DAEMON_URL: &str = "http://192.168.199.72:9090";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Config {
    Daemon { url: String, token: Option<String> },
    Mock,
    World(PathBuf),
}

impl Config {
    pub fn from_env() -> Config {
        Config::from_vars(|name| std::env::var(name).ok())
    }

    fn from_vars(lookup: impl Fn(&str) -> Option<String>) -> Config {
        if let Some(path) = lookup("TUCLAW_MOCK_WORLD") {
            return Config::World(PathBuf::from(path));
        }
        if lookup("TUCLAW_MOCK").as_deref() == Some("1") {
            return Config::Mock;
        }
        Config::Daemon {
            url: lookup("TUCLAW_DAEMON_URL").unwrap_or_else(|| DEFAULT_DAEMON_URL.to_string()),
            token: lookup("TUCLAW_CLIENT_TOKEN"),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Config::Daemon { url, token: _ } => url.clone(),
            Config::Mock => "the mock daemon".to_string(),
            Config::World(path) => path.display().to_string(),
        }
    }

    pub fn client(self) -> Result<(v3::Client, Source), String> {
        match self {
            Config::Daemon { url, token } => {
                let token = token.map(v3::ClientToken);
                let client = v3::Client::http(&url, token).map_err(|error| error.to_string())?;
                Ok((client, Source::Daemon(url)))
            }
            Config::Mock => {
                let mock = v3::MockTransport::new(v3::Scenario::default(), v3::Pace::Realtime);
                Ok((v3::Client::mock(&mock), Source::Mock))
            }
            Config::World(world) => {
                let seed = load_seed(&world)?;
                let mock =
                    v3::MockTransport::seeded(seed, v3::Scenario::default(), v3::Pace::Realtime);
                Ok((v3::Client::mock(&mock), Source::Snapshot))
            }
        }
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
    fn the_v3_1_voice_fixtures_map_to_recordings_with_transcripts() {
        let page: v3::MessagesPage = serde_json::from_str(include_str!(
            "../../core/testdata/v3/messages_page_voice.json"
        ))
        .unwrap();
        let mapped = message(&page.messages[0]);
        assert_eq!(
            mapped.voice.map(|voice| voice.mime),
            Some("audio/ogg".to_string())
        );
        assert_eq!(
            mapped.body,
            vec![Span::Text(
                "Лисички появились в магазине, что приготовить?".into()
            )]
        );
        let v3::Frame::MessageCreated(created) = v3::decode(include_str!(
            "../../core/testdata/v3/frames/message_created_voice.json"
        ))
        .unwrap() else {
            panic!("expected message.created");
        };
        let mapped = message(&created.message);
        assert_eq!(
            mapped.voice.map(|voice| voice.recording),
            Some(RecordingId(1))
        );
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

    fn of_run(kind: &str, text: &str) -> v3::Message {
        serde_json::from_value(serde_json::json!({
            "id": 9300, "surface_id": 1, "kind": kind,
            "author": {"kind": "agent", "agent_id": 1}, "text": text,
            "run_id": "f61ac42e", "created_at": "2026-10-04T11:20:00Z",
            "run_summary": {"status": "ok", "step_count": 53, "tool_count": 30, "duration_ms": 328567}
        }))
        .unwrap()
    }

    #[test]
    fn only_the_answer_carries_its_run_and_a_bare_silent_answer_has_no_text() {
        let post = message(&of_run("post", "Ответы на 5 из 6 вопросов готовы."));
        assert_eq!(post.run, None);
        assert_eq!(
            post.body,
            vec![Span::Text("Ответы на 5 из 6 вопросов готовы.".into())]
        );
        let silent = message(&of_run("answer", "[SILENT]\n"));
        assert_eq!(silent.body, vec![Span::Text(String::new())]);
        assert_eq!(silent.run.map(|run| run.tools), Some(30));
        let spoken = message(&of_run("answer", "Готово."));
        assert_eq!(spoken.body, vec![Span::Text("Готово.".into())]);
    }

    #[test]
    fn initials_take_two_words_or_two_letters() {
        assert_eq!(initials("Magnet Feed Sync"), "MF");
        assert_eq!(initials("jarvis"), "JA");
        assert_eq!(initials("Я"), "Я");
        assert_eq!(initials(""), "··");
    }

    fn vars(pairs: &[(&str, &str)]) -> Config {
        let mut owned = Vec::new();
        for (name, value) in pairs {
            owned.push((name.to_string(), value.to_string()));
        }
        let pairs = owned;
        Config::from_vars(move |name| {
            let mut found = None;
            for (key, value) in &pairs {
                if key == name {
                    found = Some(value.clone());
                }
            }
            found
        })
    }

    #[test]
    fn the_live_daemon_is_the_default_and_mocks_are_explicit() {
        assert_eq!(
            vars(&[]),
            Config::Daemon {
                url: DEFAULT_DAEMON_URL.into(),
                token: None
            }
        );
        assert_eq!(
            vars(&[
                ("TUCLAW_DAEMON_URL", "http://host:9090"),
                ("TUCLAW_CLIENT_TOKEN", "t")
            ]),
            Config::Daemon {
                url: "http://host:9090".into(),
                token: Some("t".into())
            }
        );
        assert_eq!(vars(&[("TUCLAW_MOCK", "1")]), Config::Mock);
        assert_eq!(
            vars(&[("TUCLAW_MOCK", "0")]),
            Config::Daemon {
                url: DEFAULT_DAEMON_URL.into(),
                token: None
            }
        );
        assert_eq!(
            vars(&[
                ("TUCLAW_MOCK", "1"),
                ("TUCLAW_MOCK_WORLD", "/tmp/world.json"),
                ("TUCLAW_DAEMON_URL", "http://host:9090")
            ]),
            Config::World(PathBuf::from("/tmp/world.json"))
        );
        assert_eq!(
            vars(&[
                ("TUCLAW_MOCK", "1"),
                ("TUCLAW_DAEMON_URL", "http://host:9090")
            ]),
            Config::Mock
        );
    }

    #[test]
    fn each_config_builds_its_source() {
        let Ok((_client, Source::Mock)) = Config::Mock.client() else {
            panic!("the mock starts");
        };
        let daemon = Config::Daemon {
            url: "http://192.168.1.10:9090".into(),
            token: None,
        };
        let Ok((_client, Source::Daemon(url))) = daemon.client() else {
            panic!("an open daemon needs no token");
        };
        assert_eq!(url, "http://192.168.1.10:9090");
        let bad = Config::Daemon {
            url: "ftp://host".into(),
            token: None,
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
        let seeded = Config::World(world.clone());
        let Ok((_client, Source::Snapshot)) = seeded.client() else {
            panic!("a world file means the snapshot");
        };
        std::fs::write(&world, "not json").expect("the world is overwritten");
        let broken = Config::World(world.clone());
        let Err(error) = broken.client() else {
            panic!("a broken world fails to start");
        };
        assert!(error.contains("world.json"), "{error}");
        std::fs::remove_dir_all(&directory).expect("the temporary directory is removed");
    }
}
