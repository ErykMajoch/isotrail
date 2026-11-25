/// A picture or a video by its first bytes, whatever the file's name: "an
/// image" or "a video", or none. A scene bundle may be called hike.png
pub fn media_kind(start: &[u8]) -> Option<&'static str> {
    let image = start.starts_with(&[0x89, b'P', b'N', b'G'])
        || start.starts_with(&[0xff, 0xd8, 0xff])
        || start.starts_with(b"GIF8")
        || (start.starts_with(b"RIFF") && start.get(8..12) == Some(b"WEBP"));
    if image {
        return Some("an image");
    }
    // MP4, MOV and HEIC files open with an ftyp box naming their brand
    if start.get(4..8) == Some(b"ftyp") {
        let still = matches!(
            start.get(8..12),
            Some(b"heic" | b"heix" | b"mif1" | b"avif")
        );
        return Some(if still { "an image" } else { "a video" });
    }
    // WebM and Matroska
    start
        .starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        .then_some("a video")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_and_videos_are_told_by_their_first_bytes() {
        assert_eq!(media_kind(b"\x89PNG\r\n\x1a\n"), Some("an image"));
        assert_eq!(media_kind(b"\0\0\0\x18ftypheic"), Some("an image"));
        assert_eq!(media_kind(b"\0\0\0\x18ftypisom"), Some("a video"));
        assert_eq!(media_kind(&[0x1a, 0x45, 0xdf, 0xa3, 0]), Some("a video"));
        assert_eq!(media_kind(b"<?xml version=\"1.0\"?><gpx"), None);
        assert_eq!(media_kind(b""), None);
    }
}
