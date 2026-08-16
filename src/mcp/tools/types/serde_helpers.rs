//! Custom serde deserializers for MCP tool types.
//!
//! **Scope: whole scalars only.** These helpers coerce a scalar argument that
//! a lenient client sent as the wrong JSON type — a numeric string for an
//! integer, a number for a string, `"true"`/`1` for a bool. `Vec` *elements*
//! are deliberately left strict: `"10"` inside `message_ids` is a type error,
//! not a `10`. Clients stringify whole arguments, not individual array items,
//! so per-element coercion would add surface with no matching failure mode,
//! and a mixed-type array is worth reporting rather than silently repairing.
//!
//! Leniency stops at the transport boundary. Field types and the advertised
//! `JsonSchema` are unchanged — schemars ignores `deserialize_with` — and the
//! domain layer (`params.rs`, the newtypes) stays strict.

use serde::de::Error;
use serde::{Deserialize, Deserializer};

/// Deserialize an optional string-encoded enum, treating empty strings and
/// JSON `null` as `None`. Handles MCP clients that send `"field": ""`
/// instead of omitting the field. Non-empty values parse with `T`'s own
/// `Deserialize`, so `T`'s error text is preserved.
pub fn flexible_opt_enum<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) if s.is_empty() => Ok(None),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(Error::custom),
    }
}

/// Deserialize `Option<T>` for an integer `T`, accepting either a JSON number
/// or a numeric string. The string form is trimmed before parsing. An
/// empty/whitespace string or a JSON `null` becomes `None`. Floats,
/// negatives (for unsigned `T`), out-of-range, and non-numeric values error.
pub fn flexible_opt_int<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + std::str::FromStr,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr<T> {
        Num(T),
        Str(String),
    }

    match Option::<NumOrStr<T>>::deserialize(deserializer)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => Ok(Some(n)),
        Some(NumOrStr::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed
                .parse::<T>()
                .map(Some)
                .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s)))
        }
    }
}

/// Deserialize `i64` accepting either a JSON number or a numeric string.
///
/// The string form is trimmed before parsing. Empty, non-numeric, or float
/// values produce an error.
pub fn flexible_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(i64),
        Str(String),
    }

    match NumOrStr::deserialize(deserializer)? {
        NumOrStr::Num(n) => Ok(n),
        NumOrStr::Str(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| Error::custom(format!("expected an integer, got '{}'", s))),
    }
}

/// Deserialize `String` accepting either a JSON string or an integer JSON number.
///
/// Integer numbers are stringified (`123` -> `"123"`). Float numbers error.
pub fn flexible_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrInt {
        Str(String),
        Int(i64),
    }

    match StrOrInt::deserialize(deserializer)? {
        StrOrInt::Str(s) => Ok(s),
        StrOrInt::Int(n) => Ok(n.to_string()),
    }
}

/// Deserialize `Option<String>` accepting a JSON string or an integer number.
///
/// Integer numbers are stringified. An empty/whitespace string or JSON `null`
/// becomes `None`. Float numbers error.
pub fn flexible_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrInt {
        Str(String),
        Int(i64),
    }

    match Option::<StrOrInt>::deserialize(deserializer)? {
        None => Ok(None),
        Some(StrOrInt::Str(s)) if s.trim().is_empty() => Ok(None),
        Some(StrOrInt::Str(s)) => Ok(Some(s)),
        Some(StrOrInt::Int(n)) => Ok(Some(n.to_string())),
    }
}

/// Deserialize `Option<bool>` accepting a JSON bool, the numbers `0`/`1`, or the
/// strings `"true"`/`"false"`/`"1"`/`"0"` (case-insensitive, trimmed).
///
/// An empty/whitespace string or JSON `null` becomes `None`. Anything else errors.
pub fn flexible_opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrIntOrStr {
        Bool(bool),
        Int(i64),
        Str(String),
    }

    match Option::<BoolOrIntOrStr>::deserialize(deserializer)? {
        None => Ok(None),
        Some(BoolOrIntOrStr::Bool(b)) => Ok(Some(b)),
        Some(BoolOrIntOrStr::Int(n)) => match n {
            0 => Ok(Some(false)),
            1 => Ok(Some(true)),
            other => Err(Error::custom(format!(
                "expected a boolean, got '{}'",
                other
            ))),
        },
        Some(BoolOrIntOrStr::Str(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            match trimmed.to_ascii_lowercase().as_str() {
                "true" | "1" => Ok(Some(true)),
                "false" | "0" => Ok(Some(false)),
                _ => Err(Error::custom(format!("expected a boolean, got '{}'", s))),
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/serde_helpers_tests.rs"]
mod tests;
