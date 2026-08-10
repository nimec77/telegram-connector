//! Published tool schemas must be self-contained (work-order B3).
//!
//! A dangling `#/$defs/MediaFilter` $ref shipped in 0.13.0 made media_filter
//! uncallable: schema-following clients could not construct any valid value.

use crate::mcp::server::McpServer;
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use serde_json::Value;
use std::sync::Arc;

fn test_server() -> McpServer<MockTelegramClientTrait, MockRateLimiterTrait> {
    McpServer::new(
        Arc::new(MockTelegramClientTrait::new()),
        Arc::new(MockRateLimiterTrait::new()),
    )
}

/// Collect every `$ref` string value anywhere in a schema tree.
fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get("$ref") {
                refs.push(reference.clone());
            }
            for nested in map.values() {
                collect_refs(nested, refs);
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_refs(nested, refs);
            }
        }
        _ => {}
    }
}

#[test]
fn every_tool_schema_ref_resolves_locally() {
    let tools = test_server().tools_list_result().tools;
    assert_eq!(tools.len(), 12, "expected all 12 tools to be listed");

    for tool in &tools {
        let schema = Value::Object((*tool.input_schema).clone());
        let mut refs = Vec::new();
        collect_refs(&schema, &mut refs);

        for reference in refs {
            let target = reference
                .strip_prefix("#/$defs/")
                .unwrap_or_else(|| panic!("tool {}: non-local $ref {}", tool.name, reference));
            let defs = schema.get("$defs").unwrap_or_else(|| {
                panic!("tool {}: $ref {} but no $defs block", tool.name, reference)
            });
            assert!(
                defs.get(target).is_some(),
                "tool {}: $ref {} does not resolve",
                tool.name,
                reference
            );
        }
    }
}

#[test]
fn media_filter_enum_is_inline_with_no_refs() {
    let tools = test_server().tools_list_result().tools;

    for tool_name in ["search_messages", "get_recent_messages"] {
        let tool = tools
            .iter()
            .find(|t| t.name == tool_name)
            .unwrap_or_else(|| panic!("{tool_name} tool must exist"));
        let schema = Value::Object((*tool.input_schema).clone());

        let mut refs = Vec::new();
        collect_refs(&schema, &mut refs);
        assert!(
            refs.is_empty(),
            "{tool_name}: schema must be fully inline, found $refs: {refs:?}"
        );

        let serialized = serde_json::to_string(&schema).expect("schema serializes");
        for variant in [
            "photo",
            "video",
            "photo_video",
            "document",
            "audio",
            "voice",
            "video_note",
            "gif",
            "url",
            "pinned",
        ] {
            assert!(
                serialized.contains(variant),
                "{tool_name}: media_filter variant {variant} missing from schema"
            );
        }
    }
}
