//! Normalize bare whole-value placeholders without touching TOML keys or strings.

use std::ops::Range;

pub(super) struct Normalized {
    pub source: String,
    pub placeholders: Vec<Range<usize>>,
}

#[derive(Clone, Copy)]
enum Container {
    Document,
    Array,
    InlineTable,
}

#[derive(Clone, Copy, PartialEq)]
enum Position {
    Key,
    Value,
    AfterValue,
}

/// This lexer only recognizes places where a value can start. The TOML parser
/// still owns syntax validation, including invalid keys, delimiters and nesting.
pub(super) fn normalize(source: &str) -> Normalized {
    let bytes = source.as_bytes();
    let mut output = bytes.to_vec();
    let mut placeholders = Vec::new();
    let mut stack = vec![(Container::Document, Position::Key)];
    let mut cursor = 0;
    while cursor < bytes.len() {
        let (container, position) = *stack.last().expect("document frame remains");
        match bytes[cursor] {
            b' ' | b'\t' | b'\r' => cursor += 1,
            b'\n' => {
                if matches!(container, Container::Document) {
                    stack[0].1 = Position::Key;
                }
                cursor += 1;
            }
            b'#' => {
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'"' | b'\'' => {
                cursor = string_end(bytes, cursor);
                if position == Position::Value {
                    stack.last_mut().expect("document frame remains").1 = Position::AfterValue;
                }
            }
            b'[' if matches!(container, Container::Document) && position == Position::Key => {
                // Headers cannot contain physical newlines, even in quoted names.
                // Skipping the whole header also leaves all its keys untouched.
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'=' if position == Position::Key => {
                stack.last_mut().expect("document frame remains").1 = Position::Value;
                cursor += 1;
            }
            b'[' | b'{' if position == Position::Value => {
                stack.last_mut().expect("document frame remains").1 = Position::AfterValue;
                stack.push(if bytes[cursor] == b'[' {
                    (Container::Array, Position::Value)
                } else {
                    (Container::InlineTable, Position::Key)
                });
                cursor += 1;
            }
            b']' | b'}' if stack.len() > 1 => {
                stack.pop();
                cursor += 1;
            }
            b',' if !matches!(container, Container::Document) => {
                stack.last_mut().expect("document frame remains").1 =
                    if matches!(container, Container::Array) {
                        Position::Value
                    } else {
                        Position::Key
                    };
                cursor += 1;
            }
            _ if position == Position::Value => {
                if let Some(end) = placeholder_end(bytes, cursor) {
                    // Same-width ASCII replacement preserves every original offset.
                    output[cursor..end].fill(b' ');
                    output[cursor..cursor + 2].copy_from_slice(b"\"\"");
                    placeholders.push(cursor..end);
                    cursor = end;
                } else {
                    while cursor < bytes.len() && !value_boundary(bytes[cursor]) {
                        cursor += 1;
                    }
                }
                stack.last_mut().expect("document frame remains").1 = Position::AfterValue;
            }
            _ => cursor += 1,
        }
    }
    Normalized {
        source: String::from_utf8(output).expect("only complete placeholder bytes become ASCII"),
        placeholders,
    }
}

fn value_boundary(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}' | b'#'
    )
}

fn placeholder_end(bytes: &[u8], start: usize) -> Option<usize> {
    if !bytes[start..].starts_with(b"${") {
        return None;
    }
    let mut end = start + 2;
    while end < bytes.len() && !matches!(bytes[end], b'}' | b'\r' | b'\n') {
        end += 1;
    }
    if bytes.get(end) == Some(&b'}') && bytes.get(end + 1).is_none_or(|&byte| value_boundary(byte))
    {
        Some(end + 1)
    } else {
        None
    }
}

fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let multiline = bytes.get(start..start + 3) == Some(&[quote; 3]);
    let mut cursor = start + if multiline { 3 } else { 1 };
    while cursor < bytes.len() {
        if quote == b'"' && bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
        } else if bytes[cursor] == quote {
            if !multiline {
                return cursor + 1;
            }
            let count = bytes[cursor..]
                .iter()
                .take_while(|&&byte| byte == quote)
                .count();
            cursor += count;
            if count >= 3 {
                return cursor;
            }
        } else {
            cursor += 1;
        }
    }
    cursor
}
