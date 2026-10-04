use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Image, ImageFormat};
use tuclaw_core::model::{Agent, Picture};
use tuclaw_core::v3::ImageKind;

pub const DEFAULT_NAME: &str = "You";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub name: String,
    pub picture: Option<Picture>,
}

impl Default for Me {
    fn default() -> Me {
        Me {
            name: DEFAULT_NAME.into(),
            picture: None,
        }
    }
}

pub type Gallery = HashMap<Picture, Arc<Image>>;

#[derive(Clone, Copy)]
pub struct People<'a> {
    pub agents: &'a [Agent],
    pub me: &'a Me,
    pub gallery: &'a Gallery,
}

impl People<'_> {
    pub fn picture(&self, picture: Option<&Picture>) -> Option<Arc<Image>> {
        self.gallery.get(picture?).cloned()
    }
}

pub fn decode(bytes: Vec<u8>) -> Option<Arc<Image>> {
    let format = match ImageKind::sniff(&bytes)? {
        ImageKind::Png => ImageFormat::Png,
        ImageKind::Jpeg => ImageFormat::Jpeg,
        ImageKind::Webp => ImageFormat::Webp,
    };
    Some(Arc::new(Image::from_bytes(format, bytes)))
}

#[cfg(test)]
mod tests {
    use super::{Gallery, Me, People, decode};
    use tuclaw_core::model::Picture;

    const PNG: &[u8] = include_bytes!("../../core/testdata/v3/media/avatar_agent.png");

    #[test]
    fn a_known_image_decodes_and_anything_else_does_not() {
        assert!(decode(PNG.to_vec()).is_some());
        assert!(decode(b"GIF89a....".to_vec()).is_none());
        assert!(decode(Vec::new()).is_none());
    }

    #[test]
    fn only_a_loaded_picture_is_drawn() {
        let loaded = Picture("/api/v3/agents/1/avatar?v=a".into());
        let pending = Picture("/api/v3/agents/2/avatar?v=b".into());
        let mut gallery = Gallery::new();
        gallery.insert(loaded.clone(), decode(PNG.to_vec()).expect("decodes"));
        let me = Me::default();
        let people = People {
            agents: &[],
            me: &me,
            gallery: &gallery,
        };
        assert!(people.picture(Some(&loaded)).is_some());
        assert!(people.picture(Some(&pending)).is_none());
        assert!(people.picture(None).is_none());
    }
}
