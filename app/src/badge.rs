use tuclaw_core::model::Channel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    Replies(usize),
    Unread(usize),
    Marked,
    Activity(usize),
    Nothing,
}

pub fn indicator(channel: &Channel) -> Indicator {
    if channel.replies > 0 {
        return Indicator::Replies(channel.replies);
    }
    if channel.marked && channel.unread > 0 {
        return Indicator::Unread(channel.unread);
    }
    if channel.marked {
        return Indicator::Marked;
    }
    if channel.unread > 0 {
        return Indicator::Activity(channel.unread);
    }
    Indicator::Nothing
}

pub fn needs_attention(channel: &Channel) -> bool {
    channel.unread > 0 || channel.marked
}

#[cfg(test)]
mod tests {
    use tuclaw_core::model::{Channel, ChannelId, ChannelKind};

    use super::{Indicator, indicator, needs_attention};

    fn channel(unread: usize, replies: usize, marked: bool) -> Channel {
        Channel {
            id: ChannelId(1),
            name: "General".into(),
            group: None,
            kind: ChannelKind::Channel,
            unread,
            replies,
            marked,
            sort_index: 0,
        }
    }

    #[test]
    fn replies_win_over_everything() {
        assert_eq!(indicator(&channel(5, 2, true)), Indicator::Replies(2));
    }

    #[test]
    fn a_marked_channel_with_unread_counts_them_in_accent() {
        assert_eq!(indicator(&channel(3, 0, true)), Indicator::Unread(3));
    }

    #[test]
    fn a_marked_channel_with_nothing_new_is_a_dot() {
        assert_eq!(indicator(&channel(0, 0, true)), Indicator::Marked);
    }

    #[test]
    fn plain_unread_is_activity() {
        assert_eq!(indicator(&channel(4, 0, false)), Indicator::Activity(4));
    }

    #[test]
    fn a_read_channel_shows_nothing_and_needs_no_attention() {
        assert_eq!(indicator(&channel(0, 0, false)), Indicator::Nothing);
        assert!(!needs_attention(&channel(0, 0, false)));
        assert!(needs_attention(&channel(0, 0, true)));
        assert!(needs_attention(&channel(1, 0, false)));
    }
}
