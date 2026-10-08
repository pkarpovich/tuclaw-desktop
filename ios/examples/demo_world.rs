use std::path::{Path, PathBuf};

use futures::executor::block_on;
use time::Duration;
use tuclaw_core::v3::{
    AttachmentKind, Client, Group, GroupId, Message, MessageId, MockTransport, Pace, RunDetail,
    RunId, RunStatus, RunSummary, Scenario, Seed, SeedMedia, SuggestedReplies, Surface, SurfaceId,
    TaskScope,
};

const MOVIE_NIGHT: SurfaceId = SurfaceId(10);
const NIGHT_LOG: SurfaceId = SurfaceId(11);
const LOG_LENGTH: i64 = 320;
const HOME: GroupId = GroupId(1);
const MEDIA: GroupId = GroupId(2);
const TONE: &str = "../../core/testdata/v3/media/tone.ogg";

fn main() {
    let mock = MockTransport::new(Scenario::default(), Pace::Stepped);
    let client = Client::mock(&mock);
    let mut surfaces = block_on(client.surfaces()).expect("surfaces");
    let agents = block_on(client.agents()).expect("agents");
    let me = block_on(client.me()).ok();
    let tasks = block_on(client.tasks(TaskScope::Recent)).expect("tasks");
    let mut messages = Vec::new();
    for surface in &surfaces {
        let page = block_on(client.messages(surface.id, 200)).expect("a page");
        messages.extend(page.messages);
    }
    let mut runs = Vec::new();
    let mut wanted: Vec<RunId> = Vec::new();
    for message in &messages {
        if let Some(run) = &message.run_id
            && !wanted.contains(run)
        {
            wanted.push(run.clone());
        }
    }
    for surface in &surfaces {
        if let Some(live) = &surface.live_run {
            wanted.push(live.run_id.clone());
        }
    }
    for run in wanted {
        if let Ok(detail) = block_on(client.run(&run)) {
            runs.push(detail);
        }
    }
    let mut renamed = Vec::new();
    for (index, detail) in runs.iter_mut().enumerate() {
        let stable = RunId(format!("demo-run-{}", index + 1));
        renamed.push((detail.run.id.clone(), stable.clone()));
        detail.run.id = stable;
    }
    for message in &mut messages {
        for (old, stable) in &renamed {
            if message.run_id.as_ref() == Some(old) {
                message.run_id = Some(stable.clone());
            }
        }
    }
    for surface in &mut surfaces {
        for (old, stable) in &renamed {
            if let Some(live) = &mut surface.live_run
                && &live.run_id == old
            {
                live.run_id = stable.clone();
            }
        }
    }
    let mut media = Vec::new();
    for message in &messages {
        for attachment in &message.attachments {
            match attachment.kind {
                AttachmentKind::Voice => media.push(SeedMedia {
                    id: attachment.id,
                    path: PathBuf::from(TONE),
                }),
                AttachmentKind::Unknown => {}
            }
        }
    }
    let mut template = None;
    for surface in &surfaces {
        if surface.name == "General" {
            template = Some(surface.clone());
        }
    }
    let template = template.expect("General");
    let mut next = MessageId(0);
    let mut base = time::OffsetDateTime::UNIX_EPOCH;
    for message in &messages {
        next = next.max(message.id);
        base = base.max(message.created_at);
    }
    let log_template = template.clone();
    let movie = movie_night(&messages, &mut runs, next, base);
    let asked = movie[0].id;
    surfaces.push(Surface {
        id: MOVIE_NIGHT,
        name: "Movie Night".into(),
        topic_name: "Movie Night".into(),
        display_name: None,
        sort_order: 5,
        group_id: Some(MEDIA),
        last_message_at: movie.last().map(|message| message.created_at),
        last_read_message_id: Some(asked),
        live_run: None,
        ..template
    });
    messages.extend(movie);
    let mut last = MessageId(0);
    for message in &messages {
        last = last.max(message.id);
    }
    let log = night_log(&messages, last, base);
    surfaces.push(Surface {
        id: NIGHT_LOG,
        name: "Night Log".into(),
        topic_name: "Night Log".into(),
        display_name: None,
        sort_order: 6,
        group_id: None,
        last_message_at: log.last().map(|message| message.created_at),
        last_read_message_id: log.last().map(|message| message.id),
        live_run: None,
        ..log_template
    });
    messages.extend(log);
    let general = newest_of(&messages, SurfaceId(1), 3);
    for surface in &mut surfaces {
        match surface.name.as_str() {
            "General" => surface.last_read_message_id = general,
            "Smart Home" => {
                surface.group_id = Some(HOME);
                surface.marked_unread = true;
            }
            "Magnet Feed" => surface.group_id = Some(MEDIA),
            _ => {}
        }
    }
    let seed = Seed {
        surfaces,
        agents,
        messages,
        runs,
        media,
        me,
        tasks,
        groups: vec![
            Group {
                id: HOME,
                name: "Home".into(),
                emoji: Some("🏠".into()),
                sort_order: 0,
            },
            Group {
                id: MEDIA,
                name: "Media".into(),
                emoji: Some("🎬".into()),
                sort_order: 1,
            },
        ],
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("demo")
        .join("world.json");
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder exists");
    let json = serde_json::to_string_pretty(&seed).expect("the seed serializes");
    std::fs::write(&path, json).expect("the world is written");
    println!("{}", path.display());
}

fn night_log(messages: &[Message], last: MessageId, base: time::OffsetDateTime) -> Vec<Message> {
    let MessageId(last) = last;
    let mut user = None;
    let mut post = None;
    for message in messages {
        if user.is_none() && message.run_id.is_none() && message.author.agent_id.is_none() {
            user = Some(message.clone());
        }
        if post.is_none() && message.author.agent_id.is_some() && message.run_id.is_none() {
            post = Some(message.clone());
        }
    }
    let user = user.expect("a message by the user");
    let post = post.expect("a post by an agent");
    let mut log = Vec::new();
    for index in 0..LOG_LENGTH {
        let at = base - Duration::minutes(47 * (LOG_LENGTH - index));
        let template = if index % 4 == 0 { &user } else { &post };
        let text = if index % 4 == 0 {
            format!("Проверка №{}: всё ли в порядке?", index + 1)
        } else {
            format!(
                "Запись {} из {LOG_LENGTH}: ночной обход, всё штатно.",
                index + 1
            )
        };
        log.push(Message {
            id: MessageId(last + 1 + index),
            surface_id: NIGHT_LOG,
            text,
            created_at: at,
            client_message_id: None,
            reply_to_message_id: None,
            run_id: None,
            run_summary: None,
            suggested_replies: None,
            attachments: Vec::new(),
            ..template.clone()
        });
    }
    log
}

fn newest_of(messages: &[Message], surface: SurfaceId, skip: usize) -> Option<MessageId> {
    let mut ids = Vec::new();
    for message in messages {
        if message.surface_id == surface {
            ids.push(message.id);
        }
    }
    ids.sort();
    ids.reverse();
    ids.get(skip).copied()
}

fn movie_night(
    messages: &[Message],
    runs: &mut Vec<RunDetail>,
    next: MessageId,
    base: time::OffsetDateTime,
) -> Vec<Message> {
    let MessageId(next) = next;
    let mut user = None;
    let mut answer = None;
    for message in messages {
        if user.is_none() && message.run_id.is_none() && message.author.agent_id.is_none() {
            user = Some(message.clone());
        }
        if answer.is_none() && message.run_summary.is_some() {
            answer = Some(message.clone());
        }
    }
    let user = user.expect("a message by the user");
    let answer = answer.expect("an answer with a run");
    let mut detail: RunDetail =
        serde_json::from_str(include_str!("../../core/testdata/v3/run.json")).expect("a run");
    detail.run.surface_id = Some(MOVIE_NIGHT);
    detail.run.agent_id = answer.author.agent_id.expect("an agent");
    let run = detail.run.id.clone();
    runs.push(detail);
    let asked = Message {
        id: MessageId(next + 1),
        surface_id: MOVIE_NIGHT,
        text: "Что посмотрим сегодня вечером? Хочу что-нибудь атмосферное.".into(),
        created_at: base + Duration::minutes(10),
        client_message_id: None,
        ..user.clone()
    };
    let searched = Message {
        id: MessageId(next + 2),
        surface_id: MOVIE_NIGHT,
        text: "Проверил библиотеку на NAS: из нуара есть **Touch of Evil** и **The Third Man**, \
               оба в 4K с оригинальной дорожкой."
            .into(),
        reply_to_message_id: Some(MessageId(next + 1)),
        created_at: base + Duration::minutes(11),
        run_id: Some(run),
        run_summary: Some(RunSummary {
            status: RunStatus::Ok,
            step_count: 2,
            tool_count: 1,
            duration_ms: 13_000,
        }),
        suggested_replies: None,
        ..answer.clone()
    };
    let offered = Message {
        id: MessageId(next + 3),
        surface_id: MOVIE_NIGHT,
        text: "Что включить к 22:00?".into(),
        reply_to_message_id: None,
        created_at: base + Duration::minutes(12),
        run_id: None,
        run_summary: None,
        suggested_replies: Some(SuggestedReplies {
            options: vec![
                "Touch of Evil".into(),
                "The Third Man".into(),
                "Что-нибудь новое".into(),
            ],
            open: true,
            chosen: None,
        }),
        ..answer
    };
    vec![asked, searched, offered]
}
