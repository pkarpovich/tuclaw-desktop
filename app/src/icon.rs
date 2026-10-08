use gpui::{Hsla, Pixels, Styled, Svg, svg};
use gpui_kit::assets::{IconName, icon_assets};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Icon, Sizable};

icon_assets!(
    pub Icons,
    [
        Archive,
        ArchiveRestore,
        ArrowUp,
        AtSign,
        Bot,
        Check,
        Clock,
        ChevronDown,
        ChevronLeft,
        ChevronRight,
        Copy,
        Ellipsis,
        Folder,
        Hash,
        House,
        List,
        LoaderCircle,
        Lock,
        MoveDown,
        MoveUp,
        MessageSquare,
        Mic,
        PanelLeft,
        Paperclip,
        Pause,
        Pencil,
        Play,
        Plus,
        RotateCcw,
        Search,
        Settings,
        SlidersHorizontal,
        FaceSlightlySmiling,
        Square,
        Type,
        Upload,
        User,
        Users,
        X,
        Zap,
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Send,
    Mention,
    Agents,
    Done,
    Open,
    Back,
    Closed,
    More,
    Channel,
    Pending,
    Voice,
    Sidebar,
    Attach,
    Play,
    Search,
    Settings,
    Adjust,
    Emoji,
    Stop,
    Format,
    Close,
    Lock,
    Upload,
    Reset,
    Add,
    Message,
    Automation,
    Schedule,
    Pause,
    Archive,
    Restore,
    Up,
    Down,
    Rename,
    Folder,
    Channels,
    Copy,
    Home,
    Person,
    People,
}

impl Glyph {
    pub const ALL: [Glyph; 40] = [
        Glyph::Send,
        Glyph::Mention,
        Glyph::Agents,
        Glyph::Done,
        Glyph::Open,
        Glyph::Back,
        Glyph::Closed,
        Glyph::More,
        Glyph::Channel,
        Glyph::Pending,
        Glyph::Voice,
        Glyph::Sidebar,
        Glyph::Attach,
        Glyph::Play,
        Glyph::Search,
        Glyph::Settings,
        Glyph::Adjust,
        Glyph::Emoji,
        Glyph::Stop,
        Glyph::Format,
        Glyph::Close,
        Glyph::Lock,
        Glyph::Upload,
        Glyph::Reset,
        Glyph::Add,
        Glyph::Message,
        Glyph::Automation,
        Glyph::Schedule,
        Glyph::Pause,
        Glyph::Archive,
        Glyph::Restore,
        Glyph::Up,
        Glyph::Down,
        Glyph::Rename,
        Glyph::Folder,
        Glyph::Channels,
        Glyph::Copy,
        Glyph::Home,
        Glyph::Person,
        Glyph::People,
    ];

    fn name(self) -> IconName {
        match self {
            Glyph::Send => IconName::ArrowUp,
            Glyph::Mention => IconName::AtSign,
            Glyph::Agents => IconName::Bot,
            Glyph::Done => IconName::Check,
            Glyph::Open => IconName::ChevronDown,
            Glyph::Back => IconName::ChevronLeft,
            Glyph::Closed => IconName::ChevronRight,
            Glyph::More => IconName::Ellipsis,
            Glyph::Channel => IconName::Hash,
            Glyph::Pending => IconName::LoaderCircle,
            Glyph::Voice => IconName::Mic,
            Glyph::Sidebar => IconName::PanelLeft,
            Glyph::Attach => IconName::Paperclip,
            Glyph::Play => IconName::Play,
            Glyph::Search => IconName::Search,
            Glyph::Settings => IconName::Settings,
            Glyph::Adjust => IconName::SlidersHorizontal,
            Glyph::Emoji => IconName::FaceSlightlySmiling,
            Glyph::Stop => IconName::Square,
            Glyph::Format => IconName::Type,
            Glyph::Close => IconName::X,
            Glyph::Lock => IconName::Lock,
            Glyph::Upload => IconName::Upload,
            Glyph::Reset => IconName::RotateCcw,
            Glyph::Add => IconName::Plus,
            Glyph::Message => IconName::MessageSquare,
            Glyph::Automation => IconName::Zap,
            Glyph::Schedule => IconName::Clock,
            Glyph::Pause => IconName::Pause,
            Glyph::Archive => IconName::Archive,
            Glyph::Restore => IconName::ArchiveRestore,
            Glyph::Up => IconName::MoveUp,
            Glyph::Down => IconName::MoveDown,
            Glyph::Rename => IconName::Pencil,
            Glyph::Folder => IconName::Folder,
            Glyph::Channels => IconName::List,
            Glyph::Copy => IconName::Copy,
            Glyph::Home => IconName::House,
            Glyph::Person => IconName::User,
            Glyph::People => IconName::Users,
        }
    }
}

pub fn icon(glyph: Glyph, size: Pixels, color: Hsla) -> Svg {
    svg()
        .path(glyph.name().path())
        .flex_none()
        .size(size)
        .text_color(color)
}

pub fn spinner(size: Pixels, color: Hsla) -> Spinner {
    Spinner::new()
        .icon(Icon::new(Glyph::Pending.name()))
        .color(color)
        .with_size(size)
}

#[cfg(test)]
mod tests {
    use gpui::AssetSource;

    use super::{Glyph, Icons};

    #[test]
    fn every_glyph_is_embedded() {
        for glyph in Glyph::ALL {
            let path = glyph.name().path();
            let loaded = Icons.load(&path).expect("the source answers");
            assert!(loaded.is_some(), "{glyph:?} at {path} is not embedded");
        }
    }

    #[test]
    fn every_glyph_names_its_own_icon() {
        let mut seen = Vec::new();
        for glyph in Glyph::ALL {
            let path = glyph.name().path();
            assert!(!seen.contains(&path), "{glyph:?} repeats {path}");
            seen.push(path);
        }
    }
}
