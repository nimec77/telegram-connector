//! Bounded container/signature inspection, never codec validation or full image decoding.
mod reader;

use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailure, SendFailureKind, SendMediaKind};
use reader::Reader;
use std::io::{Seek as _, SeekFrom};

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
    snapshot_len: u64,
    deadline: SendDeadline,
) -> Result<ProbedMedia, SendFailure> {
    let original = file.stream_position().map_err(|_| unsupported())?;
    let result = (|| {
        if deadline.expired() {
            return Err(SendFailure::not_sent(SendFailureKind::Timeout, None));
        }
        file.rewind().map_err(|_| unsupported())?;
        inspect(
            &mut Reader::new(file, snapshot_len, deadline),
            kind,
            snapshot_len,
        )
    })();
    // Restoration must run even when inspection fails or its original budget expires.
    file.seek(SeekFrom::Start(original))
        .map_err(|_| unsupported())?;
    if deadline.expired() {
        return Err(SendFailure::not_sent(SendFailureKind::Timeout, None));
    }
    result
}

fn inspect(
    reader: &mut Reader<'_>,
    kind: &SendMediaKind,
    len: u64,
) -> Result<ProbedMedia, SendFailure> {
    let prefix = reader.read::<2>()?;
    let (mime_type, extension) = if matches!(kind, SendMediaKind::Image) && prefix == [0xff, 0xd8] {
        jpeg(reader)?;
        ("image/jpeg", "jpg")
    } else {
        let mut bytes = prefix.to_vec();
        if matches!(kind, SendMediaKind::Audio { .. }) {
            // ID3's ten-byte header is the only MP3 metadata we read before skipping its body.
            let head_len = len.min(10) as usize;
            bytes.resize(head_len, 0);
            reader.read_into(&mut bytes[2..])?;
            if bytes.starts_with(b"ID3") {
                mp3(reader, &bytes)?;
                return Ok(ProbedMedia {
                    mime_type: "audio/mpeg",
                    extension: "mp3",
                });
            }
            if mpeg(&bytes) {
                return Ok(ProbedMedia {
                    mime_type: "audio/mpeg",
                    extension: "mp3",
                });
            }
        }
        // The other existing signature/container checks retain their fixed-prefix behavior.
        let prefix_len = len.min(reader::READ_LIMIT as u64) as usize;
        let inspected = bytes.len();
        bytes.resize(prefix_len, 0);
        reader.read_into(&mut bytes[inspected..])?;
        match kind {
            SendMediaKind::Image if png(&bytes) => ("image/png", "png"),
            SendMediaKind::Audio { .. } if aac(&bytes) => ("audio/aac", "aac"),
            SendMediaKind::Audio { .. } if mp4(&bytes, false) => ("audio/mp4", "m4a"),
            SendMediaKind::Voice { .. } if opus(&bytes) => ("audio/ogg", "ogg"),
            SendMediaKind::Video { .. } if mp4(&bytes, true) => ("video/mp4", "mp4"),
            _ => return Err(unsupported()),
        }
    };
    reader.check()?;
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

fn jpeg(reader: &mut Reader<'_>) -> Result<(), SendFailure> {
    let mut dimensions = false;
    // SOI already consumed; include SOI, SOF and SOS in the independent marker limit.
    for _ in 1..4096 {
        if reader.read::<1>()? != [0xff] {
            return Err(unsupported());
        }
        let marker = loop {
            let [byte] = reader.read::<1>()?;
            if byte != 0xff {
                break byte;
            }
        };
        if marker == 1 {
            continue;
        } // TEM is the only standalone marker before a scan.
        if !(0xc0..=0xfe).contains(&marker) || (0xd0..=0xd9).contains(&marker) {
            return Err(unsupported());
        }
        let size = u16::from_be_bytes(reader.read::<2>()?);
        let payload = size.checked_sub(2).ok_or_else(unsupported)?;
        let end = reader.end(u64::from(payload))?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if payload < 6 || dimensions {
                return Err(unsupported());
            }
            let header = reader.read::<6>()?;
            if header[1..3] == [0, 0]
                || header[3..5] == [0, 0]
                || header[5] == 0
                || payload != 6 + 3 * u16::from(header[5])
            {
                return Err(unsupported());
            }
            dimensions = true;
        } else if marker == 0xda {
            if !dimensions || payload < 4 {
                return Err(unsupported());
            }
            let [components] = reader.read::<1>()?;
            if components == 0 || payload != 4 + 2 * u16::from(components) {
                return Err(unsupported());
            }
            reader.seek(end)?;
            return Ok(());
        }
        // Includes APP/COM metadata and bounded non-image tables; no payload decoding.
        reader.seek(end)?;
    }
    Err(unsupported())
}

fn synchsafe(bytes: [u8; 4]) -> Result<u64, SendFailure> {
    if bytes.iter().any(|byte| byte & 0x80 != 0) {
        return Err(unsupported());
    }
    Ok(bytes
        .into_iter()
        .fold(0, |size, byte| (size << 7) | u64::from(byte)))
}

fn mp3(reader: &mut Reader<'_>, header: &[u8]) -> Result<(), SendFailure> {
    if header.len() != 10 || header[4] == 0xff {
        return Err(unsupported());
    }
    let mask = match header[3] {
        2 => 0xc0,
        3 => 0xe0,
        4 => 0xf0,
        _ => return Err(unsupported()),
    };
    if header[5] & !mask != 0 {
        return Err(unsupported());
    }
    let size = synchsafe([header[6], header[7], header[8], header[9]])?;
    let body_end = reader.end(size)?;
    if header[3] >= 3 && header[5] & 0x40 != 0 {
        if size < 4 {
            return Err(unsupported());
        }
        let raw = reader.read::<4>()?;
        let extended = if header[3] == 4 {
            synchsafe(raw)?
        } else {
            u64::from(u32::from_be_bytes(raw)) + 4
        };
        let minimum = if header[3] == 4 { 6 } else { 10 };
        if extended < minimum || extended > size {
            return Err(unsupported());
        }
    }
    reader.seek(body_end)?;
    if header[3] == 4 && header[5] & 0x10 != 0 {
        // v2.4 declares body size excluding BOTH the header and the optional footer.
        let footer = reader.read::<10>()?;
        if &footer[..3] != b"3DI" || footer[3..] != header[3..] {
            return Err(unsupported());
        }
    }
    if !mpeg(&reader.read::<4>()?) {
        return Err(unsupported());
    }
    Ok(())
}
fn mpeg(bytes: &[u8]) -> bool {
    bytes.get(..4).is_some_and(|h| {
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
