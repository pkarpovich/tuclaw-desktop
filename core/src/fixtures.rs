use time::{Duration, OffsetDateTime, Time};

use crate::model::{Agent, AgentId, AgentStatus, Author, Channel, ChannelId, ChannelKind, Span};

const MAGNET: AgentId = AgentId(1);
const ALLSPEAK: AgentId = AgentId(2);
const REVIEW: AgentId = AgentId(3);
const GENERAL: AgentId = AgentId(4);

const MOVIE_NIGHT: ChannelId = ChannelId(1);
const MEDIA_ARCHIVE: ChannelId = ChannelId(2);
const APARTMENT_RENO: ChannelId = ChannelId(3);
const SMART_HOME: ChannelId = ChannelId(4);
const DOWNLOADS: ChannelId = ChannelId(5);
const PERSONAL: ChannelId = ChannelId(6);
const DIRECT_GENERAL: ChannelId = ChannelId(7);
const DIRECT_MAGNET: ChannelId = ChannelId(8);
const DIRECT_ALLSPEAK: ChannelId = ChannelId(9);
const DIRECT_REVIEW: ChannelId = ChannelId(10);

pub struct Fixtures {
    pub agents: Vec<Agent>,
    pub channels: Vec<Channel>,
    pub conversations: Vec<Conversation>,
}

pub struct Conversation {
    pub channel: ChannelId,
    pub messages: Vec<SeedMessage>,
}

pub struct SeedMessage {
    pub author: Author,
    pub body: Vec<Span>,
    pub sent_at: OffsetDateTime,
    pub replies: Vec<SeedReply>,
}

pub struct SeedReply {
    pub author: Author,
    pub body: Vec<Span>,
    pub sent_at: OffsetDateTime,
}

pub fn fixtures(now: OffsetDateTime) -> Fixtures {
    let timeline = Timeline { now };
    Fixtures {
        agents: agents(),
        channels: channels(),
        conversations: vec![
            Conversation {
                channel: MOVIE_NIGHT,
                messages: movie_night(&timeline),
            },
            Conversation {
                channel: MEDIA_ARCHIVE,
                messages: media_archive(&timeline),
            },
            Conversation {
                channel: APARTMENT_RENO,
                messages: apartment_reno(&timeline),
            },
            Conversation {
                channel: SMART_HOME,
                messages: smart_home(&timeline),
            },
            Conversation {
                channel: DOWNLOADS,
                messages: downloads(&timeline),
            },
            Conversation {
                channel: PERSONAL,
                messages: Vec::new(),
            },
            Conversation {
                channel: DIRECT_GENERAL,
                messages: direct_general(&timeline),
            },
            Conversation {
                channel: DIRECT_MAGNET,
                messages: direct_magnet(&timeline),
            },
            Conversation {
                channel: DIRECT_ALLSPEAK,
                messages: direct_allspeak(&timeline),
            },
            Conversation {
                channel: DIRECT_REVIEW,
                messages: direct_review(&timeline),
            },
        ],
    }
}

struct Timeline {
    now: OffsetDateTime,
}

impl Timeline {
    fn at(&self, days_ago: i64, hour: u8, minute: u8) -> OffsetDateTime {
        let at = self.now - Duration::days(days_ago);
        let Ok(time) = Time::from_hms(hour, minute, 0) else {
            return at;
        };
        at.replace_time(time)
    }
}

fn agents() -> Vec<Agent> {
    vec![
        Agent {
            id: MAGNET,
            name: "magnet feed sync".to_string(),
            initials: "mf".to_string(),
            role: "Watches feeds and pulls files down".to_string(),
            status: AgentStatus::Busy("Waiting for the download to finish".to_string()),
            sort_index: 0,
        },
        Agent {
            id: ALLSPEAK,
            name: "allspeak".to_string(),
            initials: "as".to_string(),
            role: "Syncs and translates subtitles".to_string(),
            status: AgentStatus::Busy("Syncing subtitles for tonight".to_string()),
            sort_index: 1,
        },
        Agent {
            id: REVIEW,
            name: "media review".to_string(),
            initials: "mr".to_string(),
            role: "Logs your take on films, shows and games".to_string(),
            status: AgentStatus::Idle,
            sort_index: 2,
        },
        Agent {
            id: GENERAL,
            name: "tuclaw general".to_string(),
            initials: "tg".to_string(),
            role: "Smart home, everything else, coordinates the others".to_string(),
            status: AgentStatus::Idle,
            sort_index: 3,
        },
    ]
}

fn channels() -> Vec<Channel> {
    let movies = "🎬 Movie nights";
    let home = "🏠 Home";
    vec![
        Channel {
            id: MOVIE_NIGHT,
            name: "movie-night".to_string(),
            group: Some(movies.to_string()),
            kind: ChannelKind::Channel,
            unread: 0,
            sort_index: 0,
        },
        Channel {
            id: MEDIA_ARCHIVE,
            name: "media-archive".to_string(),
            group: Some(movies.to_string()),
            kind: ChannelKind::Channel,
            unread: 0,
            sort_index: 1,
        },
        Channel {
            id: APARTMENT_RENO,
            name: "apartment-reno".to_string(),
            group: Some(movies.to_string()),
            kind: ChannelKind::Channel,
            unread: 0,
            sort_index: 2,
        },
        Channel {
            id: SMART_HOME,
            name: "smart-home".to_string(),
            group: Some(home.to_string()),
            kind: ChannelKind::Channel,
            unread: 0,
            sort_index: 3,
        },
        Channel {
            id: DOWNLOADS,
            name: "downloads".to_string(),
            group: Some(home.to_string()),
            kind: ChannelKind::Channel,
            unread: 0,
            sort_index: 4,
        },
        Channel {
            id: PERSONAL,
            name: "personal".to_string(),
            group: None,
            kind: ChannelKind::Channel,
            unread: 0,
            sort_index: 5,
        },
        Channel {
            id: DIRECT_GENERAL,
            name: "tuclaw general".to_string(),
            group: None,
            kind: ChannelKind::Direct(GENERAL),
            unread: 0,
            sort_index: 6,
        },
        Channel {
            id: DIRECT_MAGNET,
            name: "magnet feed sync".to_string(),
            group: None,
            kind: ChannelKind::Direct(MAGNET),
            unread: 2,
            sort_index: 7,
        },
        Channel {
            id: DIRECT_ALLSPEAK,
            name: "allspeak".to_string(),
            group: None,
            kind: ChannelKind::Direct(ALLSPEAK),
            unread: 0,
            sort_index: 8,
        },
        Channel {
            id: DIRECT_REVIEW,
            name: "media review".to_string(),
            group: None,
            kind: ChannelKind::Direct(REVIEW),
            unread: 0,
            sort_index: 9,
        },
    ]
}

fn text(body: &str) -> Vec<Span> {
    vec![Span::Text(body.to_string())]
}

fn user(sent_at: OffsetDateTime, body: Vec<Span>) -> SeedMessage {
    SeedMessage {
        author: Author::User,
        body,
        sent_at,
        replies: Vec::new(),
    }
}

fn agent(id: AgentId, sent_at: OffsetDateTime, body: Vec<Span>) -> SeedMessage {
    SeedMessage {
        author: Author::Agent(id),
        body,
        sent_at,
        replies: Vec::new(),
    }
}

fn user_reply(sent_at: OffsetDateTime, body: Vec<Span>) -> SeedReply {
    SeedReply {
        author: Author::User,
        body,
        sent_at,
    }
}

fn agent_reply(id: AgentId, sent_at: OffsetDateTime, body: Vec<Span>) -> SeedReply {
    SeedReply {
        author: Author::Agent(id),
        body,
        sent_at,
    }
}

fn movie_night(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        user(
            timeline.at(25, 20, 14),
            text("Starting a movie night thing. Once a week, projector, no phones."),
        ),
        agent(
            MAGNET,
            timeline.at(25, 20, 16),
            text(
                "Noted. I'll watch the usual trackers and keep anything 4K with the original audio.",
            ),
        ),
        agent(
            GENERAL,
            timeline.at(25, 20, 20),
            text(
                "I'll take the room: lights, blinds, projector input. Give me a time and I'll fire the scene.",
            ),
        ),
        user(
            timeline.at(23, 21, 2),
            text("First pick should be noir. Something I have not seen."),
        ),
        agent(
            REVIEW,
            timeline.at(23, 21, 5),
            text(
                "Your archive holds 14 noir entries since 2019. I can rule those out when picks come in.",
            ),
        ),
        agent(
            MAGNET,
            timeline.at(23, 21, 9),
            text("Then I filter for noir, 4K, seeds over 20."),
        ),
        user(timeline.at(21, 19, 40), text("How is the NAS holding up?")),
        agent(
            MAGNET,
            timeline.at(21, 19, 41),
            vec![
                Span::Text("Files land in ".to_string()),
                Span::Code("/volume1/media/film".to_string()),
                Span::Text(", 3.1 TB free.".to_string()),
            ],
        ),
        user(
            timeline.at(21, 19, 45),
            text("Good. Keep the folder names clean."),
        ),
        agent(
            MAGNET,
            timeline.at(21, 19, 47),
            text("One folder per film, year in the name, subtitles beside the file."),
        ),
        user(
            timeline.at(19, 22, 10),
            text("Anything worth watching tonight?"),
        ),
        agent(
            MAGNET,
            timeline.at(19, 22, 12),
            text("Two candidates finished downloading this afternoon."),
        ),
        user(timeline.at(19, 22, 30), text("Leave them, I'm too tired.")),
        agent(
            GENERAL,
            timeline.at(17, 18, 5),
            text("Projector firmware updated. HDMI 2 is still the input for the media box."),
        ),
        user(timeline.at(17, 18, 20), text("Did that break the scene?")),
        agent(
            GENERAL,
            timeline.at(17, 18, 21),
            text("Rebuilt it. Lights to 20%, blinds down, projector on HDMI 2."),
        ),
        user(timeline.at(17, 18, 22), text("Good.")),
        user(
            timeline.at(15, 20, 0),
            text("Let's do Thursday nights from now on."),
        ),
        agent(
            GENERAL,
            timeline.at(15, 20, 1),
            text("Weekly at 21:59 on Thursdays. I'll ask before it fires the first time."),
        ),
        agent(
            REVIEW,
            timeline.at(15, 20, 4),
            text("I'll log each one after the credits."),
        ),
        user(
            timeline.at(13, 21, 15),
            text("Subtitles were out of sync last time."),
        ),
        agent(
            ALLSPEAK,
            timeline.at(13, 21, 16),
            text("That was a 25 fps source against a 23.976 file. I re-timed it."),
        ),
        user(
            timeline.at(13, 21, 18),
            text("Can you do that without me asking?"),
        ),
        agent(
            ALLSPEAK,
            timeline.at(13, 21, 20),
            text("Yes. Once a download lands I sync before you sit down."),
        ),
        user(
            timeline.at(11, 20, 30),
            text("Pull something with a decent restoration."),
        ),
        agent(
            MAGNET,
            timeline.at(11, 20, 33),
            text("Criterion transfer, 4K, 41 seeders. Starting it now."),
        ),
        agent(
            MAGNET,
            timeline.at(11, 22, 48),
            text("Done, 22.6 GB. Filed under film/1955."),
        ),
        agent(
            ALLSPEAK,
            timeline.at(11, 22, 52),
            text("RU subtitles synced and saved beside the file."),
        ),
        user(timeline.at(9, 19, 5), text("The projector was too dim.")),
        agent(
            GENERAL,
            timeline.at(9, 19, 7),
            text("The lamp is at 61% of its rated hours. I raised the scene to 35% brightness."),
        ),
        user(timeline.at(9, 19, 10), text("Order a spare lamp.")),
        agent(
            GENERAL,
            timeline.at(9, 19, 11),
            text("Queued. I'll ask before it buys anything."),
        ),
        user(timeline.at(7, 21, 40), text("What did I watch last month?")),
        agent(
            REVIEW,
            timeline.at(7, 21, 41),
            text("Six films, four of them noir. The highest rating went to The Third Man."),
        ),
        user(timeline.at(7, 21, 44), text("That sounds right.")),
        agent(
            REVIEW,
            timeline.at(7, 21, 46),
            text("I can put next week's shortlist together myself if you want."),
        ),
        user(timeline.at(6, 12, 15), text("Not this week, I'll pick.")),
        agent(
            REVIEW,
            timeline.at(6, 12, 16),
            text("Understood. I'll only log what you watch."),
        ),
        agent(
            MAGNET,
            timeline.at(6, 12, 30),
            text("Three new noir releases showed up on the feed overnight."),
        ),
        user(
            timeline.at(4, 20, 50),
            text("Anything in 4K with the original audio?"),
        ),
        agent(
            MAGNET,
            timeline.at(4, 20, 52),
            text("Two. One has a dub track baked in, so I would skip it."),
        ),
        user(timeline.at(4, 20, 55), text("Skip it.")),
        agent(
            MAGNET,
            timeline.at(4, 20, 56),
            text("Left it. The other one is queued."),
        ),
        user(timeline.at(2, 18, 30), text("Storage check.")),
        agent(
            MAGNET,
            timeline.at(2, 18, 31),
            text("2.4 TB free. Nothing deleted without asking, as usual."),
        ),
        user(timeline.at(2, 18, 35), text("Keep it that way.")),
        agent(
            ALLSPEAK,
            timeline.at(2, 19, 2),
            text("I re-synced last week's subtitles, they drifted after the re-encode."),
        ),
        agent(
            GENERAL,
            timeline.at(2, 19, 20),
            text("Scene tested end to end. Everything answers."),
        ),
        user(
            timeline.at(1, 21, 30),
            text(
                "Put something noir together for tonight. I want to sit down at 22:00 with everything already on the NAS.",
            ),
        ),
        agent(
            GENERAL,
            timeline.at(1, 21, 31),
            vec![
                Span::Text("On it. Picks and the session go to ".to_string()),
                Span::Mention("magnet feed sync".to_string()),
                Span::Text(". Lights and projector are mine.".to_string()),
            ],
        ),
        agent(
            MAGNET,
            timeline.at(1, 21, 44),
            text("Three options. All 4K, original and RU audio."),
        ),
        user(
            timeline.at(1, 21, 50),
            text("Which of these have I not seen?"),
        ),
        agent(
            REVIEW,
            timeline.at(1, 21, 52),
            text(
                "From the archive: Maltese Falcon in 2019, The Third Man in 2022. Touch of Evil isn't in there.",
            ),
        ),
        user(timeline.at(0, 9, 12), text("Taking Touch of Evil then.")),
        agent(
            MAGNET,
            timeline.at(0, 9, 13),
            vec![
                Span::Text("14.2 GB, 38 seeders, landing in ".to_string()),
                Span::Code("/volume1/media/film".to_string()),
                Span::Text(" tonight.".to_string()),
            ],
        ),
        agent(
            ALLSPEAK,
            timeline.at(0, 9, 40),
            text("Subtitles come as a separate file. I'll sync them once the download lands."),
        ),
        SeedMessage {
            author: Author::Agent(GENERAL),
            body: text(
                "Cinema scene fires at 21:59: lights to 20%, blinds down, projector on HDMI 2.",
            ),
            sent_at: timeline.at(0, 11, 5),
            replies: vec![
                user_reply(
                    timeline.at(0, 11, 20),
                    text("Make it 21:45, I want the trailers."),
                ),
                agent_reply(GENERAL, timeline.at(0, 11, 21), text("Moved to 21:45.")),
                user_reply(
                    timeline.at(0, 11, 25),
                    text("And leave the hallway light on."),
                ),
                agent_reply(
                    GENERAL,
                    timeline.at(0, 11, 26),
                    text("Hallway stays at 40%. The rest goes down."),
                ),
            ],
        },
        user(
            timeline.at(0, 11, 30),
            vec![
                Span::Text("Good. Ping ".to_string()),
                Span::Mention("media review".to_string()),
                Span::Text(" after the credits.".to_string()),
            ],
        ),
    ]
}

fn media_archive(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        user(
            timeline.at(3, 10, 0),
            text("Add everything from the old spreadsheet to the archive."),
        ),
        agent(
            REVIEW,
            timeline.at(3, 10, 5),
            text("Imported 214 entries. Ratings kept, dates kept, duplicates merged."),
        ),
        agent(
            REVIEW,
            timeline.at(1, 23, 40),
            text("Logged: Touch of Evil — noir, strong opening, sags mid."),
        ),
        user(timeline.at(1, 23, 41), text("That's fair.")),
    ]
}

fn apartment_reno(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        user(
            timeline.at(6, 9, 30),
            text("The projector wall needs repainting before the next session."),
        ),
        agent(
            GENERAL,
            timeline.at(6, 9, 35),
            text("Matte, dark grey, 8-10% reflectance. I can put a list together."),
        ),
        user(timeline.at(2, 15, 10), text("Order the paint.")),
        agent(
            GENERAL,
            timeline.at(2, 15, 12),
            text("Queued. I'll ask before it buys anything."),
        ),
    ]
}

fn smart_home(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        agent(
            GENERAL,
            timeline.at(4, 7, 0),
            text("Morning scene ran. Blinds up at 07:00, kettle on."),
        ),
        user(
            timeline.at(4, 22, 40),
            text("Move the evening scene half an hour later."),
        ),
        agent(
            GENERAL,
            timeline.at(0, 8, 15),
            text("The evening scene now fires at 22:30."),
        ),
        user(timeline.at(0, 8, 16), text("Thanks.")),
    ]
}

fn downloads(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        agent(
            MAGNET,
            timeline.at(9, 3, 12),
            text("Overnight queue finished: 4 files, 61 GB."),
        ),
        user(timeline.at(9, 9, 0), text("Anything that failed?")),
        agent(
            MAGNET,
            timeline.at(9, 9, 1),
            text("One stalled at 3% with no seeders. I dropped it."),
        ),
        agent(
            MAGNET,
            timeline.at(1, 2, 30),
            text("Touch of Evil is at 71%, forty minutes to go."),
        ),
    ]
}

fn direct_general(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        user(
            timeline.at(7, 11, 0),
            text("Can you take over the whole evening routine?"),
        ),
        agent(
            GENERAL,
            timeline.at(7, 11, 2),
            text("Yes. Lights, blinds, projector, and the kettle if you want it."),
        ),
        agent(
            GENERAL,
            timeline.at(0, 7, 45),
            text("Nothing needs you this morning. The house is quiet."),
        ),
    ]
}

fn direct_magnet(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        user(
            timeline.at(2, 16, 0),
            text("Keep at least 2 TB free at all times."),
        ),
        agent(
            MAGNET,
            timeline.at(2, 16, 1),
            text("Understood. I stop pulling below that and ask."),
        ),
        agent(
            MAGNET,
            timeline.at(0, 10, 20),
            text("The disk sits at 2.9 TB free after tonight's file."),
        ),
        agent(
            MAGNET,
            timeline.at(0, 10, 21),
            text("Waiting for the download to finish before I file it."),
        ),
    ]
}

fn direct_allspeak(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        user(
            timeline.at(13, 18, 0),
            text("Which languages can you sync?"),
        ),
        agent(
            ALLSPEAK,
            timeline.at(13, 18, 1),
            text("Anything with a timed source. Russian and English are the ones you use."),
        ),
        agent(
            ALLSPEAK,
            timeline.at(1, 22, 5),
            text("Tonight's subtitles are re-timed and saved beside the file."),
        ),
    ]
}

fn direct_review(timeline: &Timeline) -> Vec<SeedMessage> {
    vec![
        user(
            timeline.at(11, 21, 0),
            text("Rate things out of ten, not five."),
        ),
        agent(
            REVIEW,
            timeline.at(11, 21, 1),
            text("Switched. The old entries are rescaled."),
        ),
        agent(
            REVIEW,
            timeline.at(1, 23, 50),
            text("Credits are rolling. How was Touch of Evil?"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use time::macros::datetime;
    use time::{Date, OffsetDateTime};

    use super::{Conversation, Fixtures, MOVIE_NIGHT, PERSONAL, SeedMessage, fixtures};
    use crate::model::{AgentStatus, Author, ChannelId, ChannelKind, Span};

    fn now() -> OffsetDateTime {
        datetime!(2026-08-26 21:00 UTC)
    }

    fn conversation(fixtures: &Fixtures, channel: ChannelId) -> &Conversation {
        let mut found = None;
        for conversation in &fixtures.conversations {
            if conversation.channel == channel {
                found = Some(conversation);
                break;
            }
        }
        found.expect("the fixtures carry that channel")
    }

    fn dates(messages: &[SeedMessage]) -> BTreeSet<Date> {
        let mut dates = BTreeSet::new();
        for SeedMessage {
            author: _,
            body: _,
            sent_at,
            replies,
        } in messages
        {
            dates.insert(sent_at.date());
            for reply in replies {
                dates.insert(reply.sent_at.date());
            }
        }
        dates
    }

    #[test]
    fn the_four_agents_come_in_sort_order() {
        let Fixtures {
            agents,
            channels: _,
            conversations: _,
        } = fixtures(now());
        let mut names = Vec::new();
        let mut sort_indexes = Vec::new();
        for agent in &agents {
            names.push(agent.name.clone());
            sort_indexes.push(agent.sort_index);
        }
        assert_eq!(
            names,
            vec![
                "magnet feed sync".to_string(),
                "allspeak".to_string(),
                "media review".to_string(),
                "tuclaw general".to_string(),
            ]
        );
        assert_eq!(sort_indexes, vec![0, 1, 2, 3]);
        assert_eq!(
            agents[0].status,
            AgentStatus::Busy("Waiting for the download to finish".to_string())
        );
        assert_eq!(agents[2].status, AgentStatus::Idle);
        assert_eq!(agents[3].status, AgentStatus::Idle);
    }

    #[test]
    fn ten_channels_come_in_sidebar_order_with_their_groups() {
        let Fixtures {
            agents: _,
            channels,
            conversations: _,
        } = fixtures(now());
        assert_eq!(channels.len(), 10);
        let mut rows = Vec::new();
        for channel in &channels {
            rows.push((channel.name.clone(), channel.group.clone()));
        }
        assert_eq!(
            rows,
            vec![
                (
                    "movie-night".to_string(),
                    Some("🎬 Movie nights".to_string())
                ),
                (
                    "media-archive".to_string(),
                    Some("🎬 Movie nights".to_string())
                ),
                (
                    "apartment-reno".to_string(),
                    Some("🎬 Movie nights".to_string())
                ),
                ("smart-home".to_string(), Some("🏠 Home".to_string())),
                ("downloads".to_string(), Some("🏠 Home".to_string())),
                ("personal".to_string(), None),
                ("tuclaw general".to_string(), None),
                ("magnet feed sync".to_string(), None),
                ("allspeak".to_string(), None),
                ("media review".to_string(), None),
            ]
        );
        let mut directs = 0;
        for channel in &channels {
            match channel.kind {
                ChannelKind::Channel => {}
                ChannelKind::Direct(_) => directs += 1,
            }
        }
        assert_eq!(directs, 4);
    }

    #[test]
    fn exactly_one_channel_carries_an_unread_count() {
        let Fixtures {
            agents: _,
            channels,
            conversations: _,
        } = fixtures(now());
        let mut unread = Vec::new();
        for channel in &channels {
            if channel.unread > 0 {
                unread.push((channel.name.clone(), channel.unread));
            }
        }
        assert_eq!(unread, vec![("magnet feed sync".to_string(), 2)]);
    }

    #[test]
    fn movie_night_holds_fifty_eight_messages_across_fifteen_days() {
        let fixtures = fixtures(now());
        let Conversation {
            channel: _,
            messages,
        } = conversation(&fixtures, MOVIE_NIGHT);
        assert_eq!(messages.len(), 58);
        assert_eq!(dates(messages).len(), 15);
    }

    #[test]
    fn movie_night_carries_one_thread_root_with_four_replies() {
        let fixtures = fixtures(now());
        let Conversation {
            channel: _,
            messages,
        } = conversation(&fixtures, MOVIE_NIGHT);
        let mut roots = Vec::new();
        for message in messages {
            if !message.replies.is_empty() {
                roots.push(message.replies.len());
            }
        }
        assert_eq!(roots, vec![4]);
    }

    #[test]
    fn movie_night_carries_a_mention_and_a_code_span() {
        let fixtures = fixtures(now());
        let Conversation {
            channel: _,
            messages,
        } = conversation(&fixtures, MOVIE_NIGHT);
        let mut mentions = 0;
        let mut codes = 0;
        for message in messages {
            for span in &message.body {
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
    fn personal_is_the_only_empty_conversation() {
        let fixtures = fixtures(now());
        let mut empty = Vec::new();
        for Conversation { channel, messages } in &fixtures.conversations {
            if messages.is_empty() {
                empty.push(*channel);
            }
        }
        assert_eq!(empty, vec![PERSONAL]);
    }

    #[test]
    fn every_other_conversation_spans_at_least_two_days() {
        let fixtures = fixtures(now());
        for Conversation { channel, messages } in &fixtures.conversations {
            if *channel == PERSONAL {
                continue;
            }
            let ChannelId(raw) = *channel;
            assert!(
                dates(messages).len() >= 2,
                "channel {raw} spans fewer than two days"
            );
        }
    }

    #[test]
    fn the_fixtures_span_sixteen_distinct_days() {
        let fixtures = fixtures(now());
        let mut days = BTreeSet::new();
        for Conversation {
            channel: _,
            messages,
        } in &fixtures.conversations
        {
            for date in dates(messages) {
                days.insert(date);
            }
        }
        assert_eq!(days.len(), 16);
    }

    #[test]
    fn today_and_yesterday_both_carry_messages() {
        let fixtures = fixtures(now());
        let Conversation {
            channel: _,
            messages,
        } = conversation(&fixtures, MOVIE_NIGHT);
        let dates = dates(messages);
        let today = now().date();
        let yesterday = today.previous_day().expect("the date has a predecessor");
        assert!(dates.contains(&today));
        assert!(dates.contains(&yesterday));
    }

    #[test]
    fn a_message_seeded_yesterday_lands_on_the_previous_calendar_day() {
        let fixtures = fixtures(now());
        let Conversation {
            channel: _,
            messages,
        } = conversation(&fixtures, MOVIE_NIGHT);
        let yesterday = now()
            .date()
            .previous_day()
            .expect("the date has a predecessor");
        let mut found = None;
        for message in messages {
            if message.sent_at.date() == yesterday {
                found = Some(message);
                break;
            }
        }
        let message = found.expect("the fixtures carry a message from yesterday");
        assert_eq!(message.sent_at.date(), yesterday);
        assert_eq!(message.author, Author::User);
    }

    #[test]
    fn every_timestamp_derives_from_the_given_now() {
        let fixtures = fixtures(datetime!(2020-02-29 12:00 +03:00));
        let Conversation {
            channel: _,
            messages,
        } = conversation(&fixtures, MOVIE_NIGHT);
        let latest = messages[messages.len() - 1].sent_at;
        assert_eq!(latest.date(), datetime!(2020-02-29 12:00 +03:00).date());
        assert_eq!(latest.offset(), datetime!(2020-02-29 12:00 +03:00).offset());
    }
}
