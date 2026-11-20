//! Map decoded string offsets back to already-validated original TOML tokens.

/// TOML owns decoding and validation. This cursor accounts only for byte widths:
/// escapes map to their backslash, physical CRLF retains both bytes, and folding emits
/// nothing. It is used only after parsing succeeds, never to accept TOML syntax.
pub(super) fn string_offset(raw: &str, decoded_offset: usize) -> usize {
    let bytes = raw.as_bytes();
    let basic = bytes.first() == Some(&b'"');
    let multiline = bytes.len() >= 6 && bytes[..3] == [bytes[0]; 3];
    let delimiter = if multiline { 3 } else { 1 };
    let mut cursor = delimiter;
    if multiline {
        if bytes.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor += 2;
        } else if bytes.get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
    }
    let mut decoded = 0;
    while cursor < bytes.len().saturating_sub(delimiter) {
        let start = cursor;
        let width = if basic && bytes[cursor] == b'\\' {
            cursor += 1;
            match bytes[cursor] {
                b' ' | b'\t' | b'\r' | b'\n' if multiline => {
                    while bytes
                        .get(cursor)
                        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
                    {
                        cursor += 1;
                    }
                    0
                }
                b'x' | b'u' | b'U' => {
                    let digits = match bytes[cursor] {
                        b'x' => 2,
                        b'u' => 4,
                        _ => 8,
                    };
                    cursor += 1;
                    let hex = &raw[cursor..cursor + digits];
                    let character = u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .expect("TOML parser validated the Unicode escape");
                    cursor += digits;
                    character.len_utf8()
                }
                _ => {
                    cursor += 1;
                    1
                }
            }
        } else {
            let width = raw[cursor..]
                .chars()
                .next()
                .expect("cursor inside token")
                .len_utf8();
            cursor += width;
            width
        };
        if decoded_offset < decoded + width {
            return start;
        }
        decoded += width;
    }
    // Only placeholder starts are requested; this also gives safe context for an
    // empty string without inventing an offset in expanded environment data.
    cursor
}

pub(super) fn line_column(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let column = prefix.chars().rev().take_while(|&ch| ch != '\n').count() + 1;
    (line, column)
}
