use gpui::{ElementId, SharedString};
use gpui_kit::base::text::{TextView, TextViewStyle};

use crate::theme;

const OPEN: &str = "<details>";
const SUMMARY_END: &str = "</summary>";
const CLOSE: &str = "</details>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parts {
    pub thinking: Option<String>,
    pub answer: String,
}

pub fn split_thinking(text: &str) -> Parts {
    let trimmed = text.trim_start();
    let whole = Parts {
        thinking: None,
        answer: text.to_string(),
    };
    let Some(rest) = trimmed.strip_prefix(OPEN) else {
        return whole;
    };
    let Some(summary_end) = rest.find(SUMMARY_END) else {
        return whole;
    };
    let inside = &rest[summary_end + SUMMARY_END.len()..];
    let Some(close) = inside.find(CLOSE) else {
        return whole;
    };
    Parts {
        thinking: Some(inside[..close].trim().to_string()),
        answer: inside[close + CLOSE.len()..].trim().to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub alt: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Picture(Picture),
    Quote(String),
}

pub fn split_pictures(text: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut prose = String::new();
    let mut quote = String::new();
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if !fenced && let Some(inner) = quoted(line) {
            if quote.is_empty() {
                flush(&mut prose, &mut segments);
            }
            quote.push_str(inner);
            continue;
        }
        flush_quote(&mut quote, &mut segments);
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            prose.push_str(line);
            continue;
        }
        if fenced {
            prose.push_str(line);
            continue;
        }
        let Some(picture) = standalone_picture(trimmed) else {
            prose.push_str(&links_for_inline_pictures(line));
            continue;
        };
        flush(&mut prose, &mut segments);
        segments.push(Segment::Picture(picture));
    }
    flush_quote(&mut quote, &mut segments);
    flush(&mut prose, &mut segments);
    segments
}

fn quoted(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

fn flush_quote(quote: &mut String, segments: &mut Vec<Segment>) {
    if !quote.trim().is_empty() {
        segments.push(Segment::Quote(quote.trim_matches('\n').to_string()));
    }
    quote.clear();
}

fn flush(prose: &mut String, segments: &mut Vec<Segment>) {
    if !prose.trim().is_empty() {
        segments.push(Segment::Text(prose.trim_matches('\n').to_string()));
    }
    prose.clear();
}

fn standalone_picture(line: &str) -> Option<Picture> {
    let rest = line.strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let inside = rest.strip_suffix(')')?;
    let url = match inside.split_once(char::is_whitespace) {
        Some((url, title)) if title.trim().starts_with('"') => url,
        Some(_) => return None,
        None => inside,
    };
    if url.is_empty() || alt.contains(']') {
        return None;
    }
    Some(Picture {
        alt: alt.to_string(),
        url: url
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_string(),
    })
}

fn links_for_inline_pictures(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut previous = None;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        let escaped = previous == Some('\\');
        if character == '!' && !escaped && characters.peek() == Some(&'[') {
            previous = Some(character);
            continue;
        }
        out.push(character);
        previous = Some(character);
    }
    out
}

pub enum Ink {
    Body,
    Muted,
}

fn without_pictures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            out.push_str(line);
            continue;
        }
        if fenced {
            out.push_str(line);
            continue;
        }
        out.push_str(&links_for_inline_pictures(line));
    }
    out
}

pub fn markdown(id: impl Into<ElementId>, text: impl Into<SharedString>, ink: Ink) -> TextView {
    let text: SharedString = text.into();
    let text = SharedString::from(without_pictures(&text));
    let foreground = match ink {
        Ink::Body => theme::text_primary(),
        Ink::Muted => theme::text_muted(),
    };
    let style = TextViewStyle::default()
        .with_foreground(foreground)
        .with_muted_foreground(theme::text_muted())
        .with_link(theme::accent())
        .with_code_background(theme::sunken())
        .with_border(theme::border());
    TextView::markdown(id, text)
        .style(style)
        .on_link_click(|url, _event, _window, cx| {
            if url.starts_with(MENTION_SCHEME) {
                return;
            }
            cx.open_url(url);
        })
}

pub const MENTION_SCHEME: &str = "tuclaw:agent/";

pub struct Known {
    pub handle: String,
    pub agent: i64,
}

pub fn link_mentions(text: &str, known: &[Known]) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
            continue;
        }
        if fenced {
            out.push_str(line);
            continue;
        }
        out.push_str(&link_line(line, known));
    }
    out
}

fn link_line(line: &str, known: &[Known]) -> String {
    let mut out = String::new();
    let mut code = false;
    let mut previous: Option<char> = None;
    let mut letters = line.char_indices();
    while let Some((at, letter)) = letters.next() {
        if letter == '`' {
            code = !code;
        }
        let opens = previous.is_none_or(|before| before.is_whitespace() || before == '(');
        previous = Some(letter);
        if letter != '@' || code || !opens {
            out.push(letter);
            continue;
        }
        let rest = &line[at + 1..];
        let mut name = String::new();
        for next in rest.chars() {
            if next.is_alphanumeric() || next == '_' || next == '-' {
                name.push(next);
            } else {
                break;
            }
        }
        let mut agent = None;
        for candidate in known {
            if candidate.handle.to_lowercase() == name.to_lowercase() {
                agent = Some(candidate.agent);
            }
        }
        let Some(agent) = agent.filter(|_| !name.is_empty()) else {
            out.push(letter);
            continue;
        };
        out.push_str(&format!("[@{name}]({MENTION_SCHEME}{agent})"));
        for _ in 0..name.chars().count() {
            letters.next();
        }
        previous = name.chars().last();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Vec<Known> {
        vec![
            Known {
                handle: "jarvis".into(),
                agent: 1,
            },
            Known {
                handle: "media_review".into(),
                agent: 4,
            },
        ]
    }

    #[test]
    fn a_known_mention_becomes_a_link() {
        assert_eq!(
            link_mentions("@Jarvis, ask @media_review about it", &known()),
            "[@Jarvis](tuclaw:agent/1), ask [@media_review](tuclaw:agent/4) about it"
        );
    }

    #[test]
    fn unknown_names_mail_addresses_and_code_stay_as_they_are() {
        assert_eq!(
            link_mentions("mail me@jarvis or @nobody, `@jarvis`", &known()),
            "mail me@jarvis or @nobody, `@jarvis`"
        );
        assert_eq!(
            link_mentions("```\n@jarvis\n```\n@jarvis", &known()),
            "```\n@jarvis\n```\n[@jarvis](tuclaw:agent/1)"
        );
    }

    #[test]
    fn a_picture_on_its_own_line_becomes_its_own_segment() {
        let text = "Here it is:\n\n![A turtle on the beach](https://s3-hub.example/media/t.jpg)\n\nNice, right?";
        assert_eq!(
            split_pictures(text),
            vec![
                Segment::Text("Here it is:".into()),
                Segment::Picture(Picture {
                    alt: "A turtle on the beach".into(),
                    url: "https://s3-hub.example/media/t.jpg".into(),
                }),
                Segment::Text("Nice, right?".into()),
            ]
        );
    }

    #[test]
    fn a_titled_picture_keeps_its_url_and_code_stays_code() {
        let text =
            "![x](https://a.example/p.png \"title\")\n```\n![not](https://a.example/q.png)\n```";
        let segments = split_pictures(text);
        assert_eq!(
            segments[0],
            Segment::Picture(Picture {
                alt: "x".into(),
                url: "https://a.example/p.png".into(),
            })
        );
        assert_eq!(
            segments[1],
            Segment::Text("```\n![not](https://a.example/q.png)\n```".into())
        );
    }

    #[test]
    fn a_picture_inside_a_sentence_reads_as_a_link() {
        assert_eq!(
            split_pictures("see ![this](https://a.example/p.png) now, \\![kept]"),
            vec![Segment::Text(
                "see [this](https://a.example/p.png) now, \\![kept]".into()
            )]
        );
        assert_eq!(
            split_pictures("plain text"),
            vec![Segment::Text("plain text".into())]
        );
    }

    #[test]
    fn markdown_never_carries_a_picture_outside_code() {
        assert_eq!(
            without_pictures("![a](file:///etc/x.png)\n```\n![b](c)\n```\n"),
            "[a](file:///etc/x.png)\n```\n![b](c)\n```\n"
        );
    }

    #[test]
    fn a_quote_becomes_its_own_segment_without_its_markers() {
        let text = "Steven answered:\n\n> Also, everything urgent is done.\n> Mark tickets with the flag.\n\nNothing else to add.";
        assert_eq!(
            split_pictures(text),
            vec![
                Segment::Text("Steven answered:".into()),
                Segment::Quote(
                    "Also, everything urgent is done.\nMark tickets with the flag.".into()
                ),
                Segment::Text("Nothing else to add.".into()),
            ]
        );
    }

    #[test]
    fn a_quote_marker_inside_code_stays_code() {
        let text = "```\n> not a quote\n```";
        assert_eq!(split_pictures(text), vec![Segment::Text(text.into())]);
    }

    #[test]
    fn a_leading_thinking_fold_is_split_from_the_answer() {
        let text = "<details><summary>Thinking</summary>\n\n- checked the notes\n- found it\n\n</details>\n\nГотово, **сэр**.";
        assert_eq!(
            split_thinking(text),
            Parts {
                thinking: Some("- checked the notes\n- found it".to_string()),
                answer: "Готово, **сэр**.".to_string(),
            }
        );
    }

    #[test]
    fn text_without_a_fold_is_all_answer() {
        for text in [
            "plain answer",
            "<details>no summary close",
            "<details><summary>x</summary> never closed",
            "answer first <details><summary>x</summary>y</details>",
        ] {
            assert_eq!(
                split_thinking(text),
                Parts {
                    thinking: None,
                    answer: text.to_string(),
                }
            );
        }
    }
}
