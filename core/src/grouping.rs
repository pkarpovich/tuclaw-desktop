use time::macros::format_description;
use time::{Date, OffsetDateTime, UtcOffset};

use crate::model::Message;

/// One day's worth of messages, under the title the feed draws as its separator.
#[derive(Debug, Clone, PartialEq)]
pub struct DaySection {
    /// The local calendar day the section covers.
    pub date: Date,
    /// The separator label: `Today`, `Yesterday`, or a formatted date.
    pub title: String,
    /// The messages sent on that day, in their input order.
    pub messages: Vec<Message>,
}

/// Groups messages into day sections, oldest day first.
///
/// The calendar day of a message is the day it falls on in `offset`, and the
/// `Today` and `Yesterday` titles are relative to `now`. Neither the local
/// timezone nor the wall clock is read, so a caller decides both.
///
/// Messages keep their input order inside a section.
///
/// # Examples
///
/// ```
/// use time::macros::{datetime, offset};
/// use tuclaw_core::grouping::group_by_day;
/// use tuclaw_core::model::{Author, Message, MessageId, Span};
///
/// let message = Message {
///     id: MessageId(1),
///     author: Author::User,
///     body: vec![Span::Text("on it".to_string())],
///     sent_at: datetime!(2026-08-26 09:00 UTC),
///     reply_count: 0,
/// };
/// let sections = group_by_day(&[message], offset!(UTC), datetime!(2026-08-26 21:00 UTC));
/// assert_eq!(sections.len(), 1);
/// assert_eq!(sections[0].title, "Today");
/// ```
pub fn group_by_day(
    messages: &[Message],
    offset: UtcOffset,
    now: OffsetDateTime,
) -> Vec<DaySection> {
    let today = now.to_offset(offset).date();
    let mut sections: Vec<DaySection> = Vec::new();
    for message in messages {
        let date = message.sent_at.to_offset(offset).date();
        let mut found = None;
        for (position, section) in sections.iter().enumerate() {
            if section.date == date {
                found = Some(position);
                break;
            }
        }
        let position = match found {
            Some(position) => position,
            None => {
                sections.push(DaySection {
                    date,
                    title: day_title(date, today),
                    messages: Vec::new(),
                });
                sections.len() - 1
            }
        };
        sections[position].messages.push(message.clone());
    }
    sections.sort_by_key(|section| section.date);
    sections
}

fn day_title(date: Date, today: Date) -> String {
    if date == today {
        return "Today".to_string();
    }
    if today.previous_day() == Some(date) {
        return "Yesterday".to_string();
    }
    let description = format_description!("[weekday], [day padding:none] [month repr:long]");
    match date.format(&description) {
        Ok(title) => title,
        Err(_) => date.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use time::macros::{date, datetime, offset};
    use time::{Duration, OffsetDateTime};

    use super::group_by_day;
    use crate::model::{Author, Message, MessageId, Span};

    fn message(id: i64, sent_at: OffsetDateTime) -> Message {
        Message {
            id: MessageId(id),
            author: Author::User,
            body: vec![Span::Text(format!("message {id}"))],
            sent_at,
            reply_count: 0,
        }
    }

    fn ids(messages: &[Message]) -> Vec<i64> {
        let mut ids = Vec::new();
        for message in messages {
            let MessageId(raw) = message.id;
            ids.push(raw);
        }
        ids
    }

    #[test]
    fn empty_input_groups_into_no_sections() {
        let sections = group_by_day(&[], offset!(UTC), datetime!(2026-08-26 12:00 UTC));
        assert!(sections.is_empty());
    }

    #[test]
    fn messages_from_today_share_one_section_in_input_order() {
        let now = datetime!(2026-08-26 21:00 UTC);
        let messages = vec![
            message(1, datetime!(2026-08-26 08:15 UTC)),
            message(2, datetime!(2026-08-26 09:30 UTC)),
            message(3, datetime!(2026-08-26 20:05 UTC)),
        ];
        let sections = group_by_day(&messages, offset!(UTC), now);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "Today");
        assert_eq!(sections[0].date, date!(2026 - 08 - 26));
        assert_eq!(ids(&sections[0].messages), vec![1, 2, 3]);
    }

    #[test]
    fn today_and_yesterday_are_separate_sections_oldest_first() {
        let now = datetime!(2026-08-26 21:00 UTC);
        let messages = vec![
            message(1, datetime!(2026-08-25 18:00 UTC)),
            message(2, datetime!(2026-08-26 09:00 UTC)),
            message(3, datetime!(2026-08-25 19:00 UTC)),
        ];
        let sections = group_by_day(&messages, offset!(UTC), now);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Yesterday");
        assert_eq!(ids(&sections[0].messages), vec![1, 3]);
        assert_eq!(sections[1].title, "Today");
        assert_eq!(ids(&sections[1].messages), vec![2]);
    }

    #[test]
    fn older_days_carry_formatted_titles_in_ascending_order() {
        let now = datetime!(2026-08-26 21:00 UTC);
        let messages = vec![
            message(1, datetime!(2026-08-26 10:00 UTC)),
            message(2, datetime!(2026-08-20 10:00 UTC)),
            message(3, datetime!(2026-08-22 10:00 UTC)),
            message(4, datetime!(2026-08-25 10:00 UTC)),
        ];
        let sections = group_by_day(&messages, offset!(UTC), now);
        let mut titles = Vec::new();
        for section in &sections {
            titles.push(section.title.clone());
        }
        assert_eq!(
            titles,
            vec![
                "Thursday, 20 August".to_string(),
                "Saturday, 22 August".to_string(),
                "Yesterday".to_string(),
                "Today".to_string(),
            ]
        );
        assert_eq!(ids(&sections[0].messages), vec![2]);
        assert_eq!(ids(&sections[3].messages), vec![1]);
    }

    #[test]
    fn local_midnight_splits_two_messages_into_different_sections() {
        let offset = offset!(+03:00);
        let now = datetime!(2026-08-26 12:00 +03:00);
        let before = datetime!(2026-08-25 23:50 +03:00);
        let after = before + Duration::minutes(20);
        let sections = group_by_day(&[message(1, before), message(2, after)], offset, now);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].date, date!(2026 - 08 - 25));
        assert_eq!(sections[0].title, "Yesterday");
        assert_eq!(sections[1].date, date!(2026 - 08 - 26));
        assert_eq!(sections[1].title, "Today");
    }

    #[test]
    fn the_offset_decides_which_day_a_message_lands_on() {
        let now = datetime!(2026-08-26 12:00 UTC);
        let messages = vec![message(1, datetime!(2026-08-25 23:30 UTC))];
        let utc = group_by_day(&messages, offset!(UTC), now);
        assert_eq!(utc[0].date, date!(2026 - 08 - 25));
        assert_eq!(utc[0].title, "Yesterday");
        let ahead = group_by_day(&messages, offset!(+03:00), now);
        assert_eq!(ahead[0].date, date!(2026 - 08 - 26));
        assert_eq!(ahead[0].title, "Today");
    }
}
