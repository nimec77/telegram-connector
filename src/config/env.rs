//! Interpolate decoded TOML values once, preserving original spans and structure.

use super::Config;
use serde::Deserialize;
use std::borrow::Cow;
use std::env::VarError;
use std::ops::Range;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

mod lexer;
mod position;
mod scalar;

/// Pure parsing seam. Environment access is supplied by the caller; replacement
/// data never becomes document syntax, and keys are never interpolated.
pub(super) fn parse_config_with(
    content: &str,
    mut lookup: impl FnMut(&str) -> Result<String, VarError>,
) -> anyhow::Result<Config> {
    let normalized = lexer::normalize(content);
    let mut root =
        DeTable::parse(&normalized.source).map_err(|error| parse_error(content, error.span()))?;
    let mut replacements = Vec::new();
    collect_table(
        root.get_ref(),
        &[],
        content,
        &normalized.placeholders,
        &mut lookup,
        &mut replacements,
    )?;
    // make_owned changes storage, not the parser type's lifetime. Retain the
    // expansion buffer until typed deserialization has consumed the tree.
    apply_table(root.get_mut(), &mut replacements.iter(), content)?;
    Config::deserialize(toml::Deserializer::from(root))
        .map_err(|error| parse_error(content, error.span()))
}

fn collect_table(
    table: &DeTable<'_>,
    path: &[&str],
    source: &str,
    bare: &[Range<usize>],
    lookup: &mut impl FnMut(&str) -> Result<String, VarError>,
    replacements: &mut Vec<Replacement>,
) -> anyhow::Result<()> {
    for (key, node) in table.iter() {
        // Keys can be owned decoded strings. Borrow them only for this recursion.
        let mut child_path = path.to_vec();
        child_path.push(key.get_ref().as_ref());
        collect_node(node, &child_path, source, bare, lookup, replacements)?;
    }
    Ok(())
}

fn collect_node(
    node: &Spanned<DeValue<'_>>,
    path: &[&str],
    source: &str,
    bare: &[Range<usize>],
    lookup: &mut impl FnMut(&str) -> Result<String, VarError>,
    replacements: &mut Vec<Replacement>,
) -> anyhow::Result<()> {
    let span = node.span();
    match node.get_ref() {
        DeValue::Table(table) => collect_table(table, path, source, bare, lookup, replacements),
        DeValue::Array(array) => {
            // An array item is not the scalar field named by its parent path.
            let mut child_path = path.to_vec();
            child_path.push("");
            for child in array.iter() {
                collect_node(child, &child_path, source, bare, lookup, replacements)?;
            }
            Ok(())
        }
        DeValue::String(value) => {
            let bare_span = bare
                .binary_search_by_key(&span.start, |range| range.start)
                .ok()
                .map(|index| &bare[index]);
            let original = bare_span.map_or(value.as_ref(), |range| &source[range.clone()]);
            let whole = original
                .strip_prefix("${")
                .and_then(|rest| rest.strip_suffix('}'))
                .is_some_and(|name| !name.contains('}'));
            let scalar = scalar::destination(path).filter(|_| whole);
            let expanded =
                expand_string(original, source, span.clone(), bare_span.is_some(), lookup)?;
            replacements.push(Replacement {
                span: bare_span.cloned().unwrap_or(span),
                value: expanded,
                scalar,
            });
            Ok(())
        }
        _ => Ok(()),
    }
}

struct Replacement {
    span: Range<usize>,
    value: String,
    scalar: Option<scalar::Scalar>,
}

fn apply_table<'a>(
    table: &mut DeTable<'a>,
    replacements: &mut std::slice::Iter<'a, Replacement>,
    source: &str,
) -> anyhow::Result<()> {
    for (_, node) in table.iter_mut() {
        apply_node(node, replacements, source)?;
    }
    Ok(())
}

fn apply_node<'a>(
    node: &mut Spanned<DeValue<'a>>,
    replacements: &mut std::slice::Iter<'a, Replacement>,
    source: &str,
) -> anyhow::Result<()> {
    match node.get_mut() {
        DeValue::Table(table) => apply_table(table, replacements, source),
        DeValue::Array(array) => {
            for child in array.iter_mut() {
                apply_node(child, replacements, source)?;
            }
            Ok(())
        }
        DeValue::String(_) => {
            let replacement = replacements
                .next()
                .expect("one expansion per string in tree order");
            let value = if let Some(scalar) = replacement.scalar {
                let parsed = DeValue::parse(&replacement.value)
                    .map_err(|_| parse_error(source, Some(replacement.span.clone())))?;
                if !scalar.accepts(parsed.get_ref()) {
                    return Err(parse_error(source, Some(replacement.span.clone())));
                }
                let mut value = parsed.into_inner();
                value.make_owned();
                value
            } else {
                DeValue::String(Cow::Borrowed(&replacement.value))
            };
            *node = Spanned::new(replacement.span.clone(), value);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn expand_string(
    value: &str,
    source: &str,
    span: Range<usize>,
    bare: bool,
    lookup: &mut impl FnMut(&str) -> Result<String, VarError>,
) -> anyhow::Result<String> {
    let mut expanded = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative) = value[cursor..].find("${") {
        let start = cursor + relative;
        let Some(end) = value[start + 2..].find('}').map(|end| start + 2 + end) else {
            break;
        };
        expanded.push_str(&value[cursor..start]);
        let name = &value[start + 2..end];
        let replacement = lookup(name).map_err(|error| {
            // NotUnicode owns secret bytes; retain only the safe condition.
            let condition = match error {
                VarError::NotPresent => "not found",
                VarError::NotUnicode(_) => "is not valid Unicode",
            };
            let offset = span.start
                + if bare {
                    start
                } else {
                    position::string_offset(&source[span.clone()], start)
                };
            let (line, column) = position::line_column(source, offset);
            anyhow::anyhow!(
                "Environment variable '{name}' {condition} at line {line}, column {column}"
            )
        })?;
        expanded.push_str(&replacement);
        cursor = end + 1;
    }
    expanded.push_str(&value[cursor..]);
    Ok(expanded)
}

fn parse_error(source: &str, span: Option<Range<usize>>) -> anyhow::Error {
    // No parser object, message, key, excerpt or source chain is retained.
    let error = if let Some(span) = span {
        let (line, column) = position::line_column(source, span.start);
        anyhow::anyhow!("Invalid TOML at line {line}, column {column}")
    } else {
        anyhow::anyhow!("Invalid TOML (location unavailable)")
    };
    error.context("Failed to parse config.toml")
}
