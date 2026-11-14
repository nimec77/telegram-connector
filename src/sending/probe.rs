//! Bounded container/signature inspection, never codec validation or full image decoding.
use crate::telegram::types::sending::{SendFailure, SendFailureKind, SendMediaKind};
use std::io::{Read as _, Seek as _};

#[derive(Debug)]
pub(super) struct ProbedMedia {
    pub mime_type: &'static str,
    pub extension: &'static str,
}
fn unsupported() -> SendFailure {
    SendFailure::not_sent(SendFailureKind::UnsupportedMedia, None)
}

pub(super) fn probe(
    file: &mut std::fs::File,
    kind: &SendMediaKind,
) -> Result<ProbedMedia, SendFailure> {
    file.rewind().map_err(|_| unsupported())?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take(65_536)
        .read_to_end(&mut bytes)
        .map_err(|_| unsupported())?;
    file.rewind().map_err(|_| unsupported())?;
    let (mime_type, extension) = match kind {
        SendMediaKind::Image if png(&bytes) => ("image/png", "png"),
        SendMediaKind::Image if jpeg(&bytes) => ("image/jpeg", "jpg"),
        SendMediaKind::Audio { .. } if mp3(&bytes) => ("audio/mpeg", "mp3"),
        SendMediaKind::Audio { .. } if aac(&bytes) => ("audio/aac", "aac"),
        SendMediaKind::Audio { .. } if mp4(&bytes, false) => ("audio/mp4", "m4a"),
        SendMediaKind::Voice { .. } if opus(&bytes) => ("audio/ogg", "ogg"),
        SendMediaKind::Video { .. } if mp4(&bytes, true) => ("video/mp4", "mp4"),
        _ => return Err(unsupported()),
    };
    Ok(ProbedMedia {
        mime_type,
        extension,
    })
}
fn png(b: &[u8]) -> bool {
    b.len() >= 33
        && &b[..8] == b"\x89PNG\r\n\x1a\n"
        && b[8..12] == [0, 0, 0, 13]
        && &b[12..16] == b"IHDR"
        && b[16..20] != [0, 0, 0, 0]
        && b[20..24] != [0, 0, 0, 0]
}
fn jpeg(b: &[u8]) -> bool {
    if !b.starts_with(b"\xff\xd8") {
        return false;
    }
    let mut at = 2;
    while at + 4 <= b.len() {
        if b[at] != 0xff {
            return false;
        }
        while at < b.len() && b[at] == 0xff {
            at += 1;
        }
        if at >= b.len() {
            return false;
        }
        let marker = b[at];
        at += 1;
        if marker == 0xda || marker == 0xd9 {
            return false;
        }
        if marker == 1 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        if at + 2 > b.len() {
            return false;
        }
        let size = usize::from(u16::from_be_bytes([b[at], b[at + 1]]));
        if size < 2 || at + size > b.len() {
            return false;
        }
        if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
            return size >= 8 && b[at + 3..at + 5] != [0, 0] && b[at + 5..at + 7] != [0, 0];
        }
        at += size;
    }
    false
}
fn mp3(b: &[u8]) -> bool {
    let mut at = 0;
    if b.starts_with(b"ID3") {
        if b.len() < 10 || b[6..10].iter().any(|b| b & 0x80 != 0) {
            return false;
        }
        let size = b[6..10]
            .iter()
            .fold(0usize, |n, b| (n << 7) | usize::from(*b));
        at = 10 + size;
    }
    b.get(at..at + 4).is_some_and(|h| {
        h[0] == 0xff
            && h[1] & 0xe0 == 0xe0
            && h[1] & 0x18 != 0x08
            && h[1] & 0x06 != 0
            && h[2] & 0xf0 != 0xf0
            && h[2] & 0x0c != 0x0c
    })
}
fn aac(b: &[u8]) -> bool {
    b.len() >= 7 && b[0] == 0xff && b[1] & 0xf6 == 0xf0 && b[2] & 0x3c != 0x3c
}
fn mp4(b: &[u8], video: bool) -> bool {
    if b.len() < 16 || &b[4..8] != b"ftyp" {
        return false;
    }
    let size = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    if size < 16 || size > b.len() {
        return false;
    }
    let brand = &b[8..12];
    if video && matches!(brand, b"M4A " | b"M4B ") {
        return false;
    }
    matches!(
        brand,
        b"isom" | b"iso2" | b"mp41" | b"mp42" | b"avc1" | b"M4V " | b"M4A " | b"M4B "
    )
}
fn opus(b: &[u8]) -> bool {
    if b.len() < 28 || !b.starts_with(b"OggS") || b[4] != 0 {
        return false;
    }
    let segments = usize::from(b[26]);
    let start = 27 + segments;
    segments > 0
        && b.get(start..start + 19).is_some()
        && b.get(27).is_some_and(|s| *s >= 19)
        && b.get(start..start + 8) == Some(b"OpusHead")
        && b.get(start + 8).is_some_and(|v| *v <= 15)
        && b.get(start + 9).is_some_and(|c| *c > 0)
}
