use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use gpui::{Image, ImageFormat};
use tuclaw_core::model::Message;
use tuclaw_core::v3::PublicUrl;

use crate::message::source;
use crate::rich::{Segment, split_pictures, split_thinking};

pub const MAX_WIDTH: f32 = 480.;
pub const MAX_HEIGHT: f32 = 400.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    pub width: f32,
    pub height: f32,
}

pub const DESKTOP_ROOM: Room = Room {
    width: MAX_WIDTH,
    height: MAX_HEIGHT,
};

pub const PHONE_ROOM: Room = Room {
    width: 300.,
    height: 320.,
};

const KNOWN: [(image::ImageFormat, ImageFormat); 4] = [
    (image::ImageFormat::Png, ImageFormat::Png),
    (image::ImageFormat::Jpeg, ImageFormat::Jpeg),
    (image::ImageFormat::Gif, ImageFormat::Gif),
    (image::ImageFormat::WebP, ImageFormat::Webp),
];

#[derive(Clone)]
pub struct Shown {
    pub image: Arc<Image>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
pub enum Remote {
    Loading,
    Ready(Shown),
    Failed,
}

pub type Shelf = HashMap<PublicUrl, Remote>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewed {
    pub url: PublicUrl,
    pub caption: String,
}

pub fn wanted(messages: &[Message], shelf: &Shelf) -> Vec<PublicUrl> {
    let mut urls = Vec::new();
    for message in messages {
        let answer = split_thinking(&source(&message.body)).answer;
        for segment in split_pictures(&answer) {
            let Segment::Picture(picture) = segment else {
                continue;
            };
            let Some(url) = PublicUrl::parse(&picture.url) else {
                continue;
            };
            if shelf.contains_key(&url) || urls.contains(&url) {
                continue;
            }
            urls.push(url);
        }
    }
    urls
}

pub fn decode(bytes: Vec<u8>) -> Option<Shown> {
    let reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?;
    let found = reader.format()?;
    let mut format = None;
    for (theirs, ours) in KNOWN {
        if found == theirs {
            format = Some(ours);
            break;
        }
    }
    let format = format?;
    let (width, height) = reader.into_dimensions().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(Shown {
        image: Arc::new(Image::from_bytes(format, bytes)),
        width,
        height,
    })
}

pub fn fit_into(width: u32, height: u32, room: Room) -> (f32, f32) {
    let width = width as f32;
    let height = height as f32;
    let scale = (room.width / width).min(room.height / height).min(1.);
    ((width * scale).round(), (height * scale).round())
}

#[cfg(test)]
mod tests {
    use super::{DESKTOP_ROOM, MAX_HEIGHT, MAX_WIDTH, PHONE_ROOM, decode, fit_into};

    const PNG: &[u8] = include_bytes!("../../core/testdata/v3/media/avatar_agent.png");

    #[test]
    fn a_png_decodes_with_its_size() {
        let shown = decode(PNG.to_vec()).expect("a png decodes");
        assert_eq!((shown.width, shown.height), (128, 128));
        assert!(decode(b"<html>not a picture</html>".to_vec()).is_none());
    }

    #[test]
    fn a_picture_fits_the_column_without_growing() {
        assert_eq!(fit_into(1600, 900, DESKTOP_ROOM), (MAX_WIDTH, 270.));
        assert_eq!(fit_into(500, 1000, DESKTOP_ROOM), (200., MAX_HEIGHT));
        assert_eq!(fit_into(120, 80, DESKTOP_ROOM), (120., 80.));
    }

    #[test]
    fn on_a_phone_a_picture_fits_the_narrower_column() {
        assert_eq!(fit_into(1600, 900, PHONE_ROOM), (300., 169.));
        assert_eq!(fit_into(500, 1000, PHONE_ROOM), (160., 320.));
        assert_eq!(fit_into(120, 80, PHONE_ROOM), (120., 80.));
    }
}
