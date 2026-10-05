use tuclaw_core::v3::ImageKind;

pub const SIDE: i32 = 512;

pub struct Upload {
    pub kind: ImageKind,
    pub bytes: Vec<u8>,
}

#[cfg(target_os = "macos")]
pub fn prepare(bytes: Vec<u8>) -> Result<Upload, String> {
    let bytes = mac::shrink_to_png(&bytes)?;
    Ok(Upload {
        kind: ImageKind::Png,
        bytes,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn prepare(bytes: Vec<u8>) -> Result<Upload, String> {
    let Some(kind) = ImageKind::sniff(&bytes) else {
        return Err("not a png, jpeg or webp image".to_string());
    };
    Ok(Upload { kind, bytes })
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_core_foundation::{
        CFBoolean, CFData, CFDictionary, CFMutableData, CFNumber, CFRetained, CFString, CFType,
    };
    use objc2_image_io::{
        CGImageDestination, CGImageSource, kCGImageSourceCreateThumbnailFromImageAlways,
        kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize,
    };

    use super::SIDE;

    const UNREADABLE: &str = "macOS cannot read this image";

    pub fn shrink_to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
        let data = CFData::from_bytes(bytes);
        let Some(source) = (unsafe { CGImageSource::with_data(&data, None) }) else {
            return Err(UNREADABLE.to_string());
        };
        let options = thumbnail_options();
        let Some(image) = (unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) })
        else {
            return Err(UNREADABLE.to_string());
        };
        let Some(output) = CFMutableData::new(None, 0) else {
            return Err("out of memory".to_string());
        };
        let png = CFString::from_static_str("public.png");
        let Some(destination) = (unsafe { CGImageDestination::with_data(&output, &png, 1, None) })
        else {
            return Err("macOS cannot write a png".to_string());
        };
        unsafe { destination.add_image(&image, None) };
        if !unsafe { destination.finalize() } {
            return Err("the image could not be converted".to_string());
        }
        Ok(output.to_vec())
    }

    fn thumbnail_options() -> CFRetained<CFDictionary<CFString, CFType>> {
        let always = CFBoolean::new(true);
        let side = CFNumber::new_i32(SIDE);
        let keys: [&CFString; 3] = unsafe {
            [
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
            ]
        };
        let values: [&CFType; 3] = [always.as_ref(), always.as_ref(), side.as_ref()];
        CFDictionary::from_slices(&keys, &values)
    }
}

#[cfg(test)]
mod tests {
    use tuclaw_core::v3::ImageKind;

    use super::{SIDE, prepare};

    const PNG: &[u8] = include_bytes!("../../core/testdata/v3/media/avatar_agent.png");
    const AVIF: &[u8] = include_bytes!("../testdata/avatar.avif");

    fn png_side(bytes: &[u8]) -> (u32, u32) {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        (width, height)
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_avif_photo_is_uploaded_as_a_png_no_larger_than_the_side() {
        assert_eq!(ImageKind::sniff(AVIF), None);
        let upload = prepare(AVIF.to_vec()).expect("macOS reads avif");
        assert_eq!(upload.kind, ImageKind::Png);
        assert_eq!(ImageKind::sniff(&upload.bytes), Some(ImageKind::Png));
        let side = u32::try_from(SIDE).expect("positive");
        assert_eq!(png_side(&upload.bytes), (side, side));
    }

    #[test]
    fn a_small_png_keeps_its_size() {
        let upload = prepare(PNG.to_vec()).expect("a png converts");
        assert_eq!(upload.kind, ImageKind::Png);
        assert_eq!(png_side(&upload.bytes), png_side(PNG));
    }

    #[test]
    fn bytes_that_are_no_image_are_refused() {
        assert!(prepare(b"not an image at all".to_vec()).is_err());
        assert!(prepare(Vec::new()).is_err());
    }
}
