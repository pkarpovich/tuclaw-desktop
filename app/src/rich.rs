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

pub enum Ink {
    Body,
    Muted,
}

pub fn markdown(id: impl Into<ElementId>, text: impl Into<SharedString>, ink: Ink) -> TextView {
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
    TextView::markdown(id, text).style(style)
}

#[cfg(test)]
mod tests {
    use super::*;

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
