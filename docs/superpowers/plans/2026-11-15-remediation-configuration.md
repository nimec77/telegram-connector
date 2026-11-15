# Configuration Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Interpolate TOML value data without source injection and enforce finite usable token-bucket settings.

**Architecture:** Mask only unquoted whole-value placeholders with a narrow TOML lexer, preserving source width and newlines. Parse a recursive spanned DeTable using the existing TOML library, expand decoded string values once, then deserialize Config directly from the mutated tree. Apply whole-placeholder scalar coercion by destination configuration type; never reinterpret replacement bytes as document structure. Keep original source locations for safe diagnostics.

**Tech Stack:** Rust, serde, toml 1.1.8 including its existing Spanned support, injected lookup tests and subprocess loader fixtures.

**Spec:** `docs/superpowers/specs/2026-11-15-review-remediation-design.md`

## Global Constraints

- Expand `${NAME}` in value strings, including embedded and multiline strings. Preserve replacement bytes as the resulting string value.
- Ignore whole-line and inline comments outside strings. Do not expand table/key names or perform recursive expansion of substituted values.
- Coerce according to the destination scalar type; numeric-looking credentials, paths and other string fields remain strings.
- A typed whole-value replacement must be a single valid scalar, not an injected array, table, assignment or trailing document fragment.
- Keep missing/non-Unicode variable errors safe, with variable name and original line context. Discard parser excerpts, expanded values, keys and unsafe source chains.
- Incomplete `${...` syntax inside a valid string stays literal, as today.
- Do not expand tilde, change config path precedence, reject previously accepted unknown configuration fields, or mutate the process environment.
- Add no dependency. Use the existing TOML library plus a narrowly scoped syntax-aware interpolation layer.
- Require `max_tokens >= 1` and a finite positive `refill_rate`.
- Keep zero read media/download or transcription costs accepted for compatibility; retain the existing enabled-send positive-cost rules and cost-versus-capacity validation.
- Do not silently clamp or change `RateLimiter::new` compatibility semantics for callers that bypass validation.
- Validate locally on macOS, with no private configuration/session changes.

## Review Focus

- Literal and multiline quoted strings must treat inserted delimiters/newlines as data (Task 1).
- Quoted/dotted table/key names containing `${NAME}` must not trigger lookups (Task 1).
- Original line numbers must survive multiline TOML decoding and placeholder normalization (Task 1).
- Unknown fields retain their existing acceptance, while SendingConfig's existing deny_unknown_fields still rejects them (Task 1).
- Zero capacity with all weighted costs zero must fail for capacity itself (Task 2).

---

### Task 1: Interpolate parsed TOML values safely (R11)

**Files:**
- Modify: `src/config/env.rs`, `src/config/loading.rs`
- Create focused `src/config/env/` lexer/value helpers if needed, with no mod.rs; keep responsibilities separate
- Test: `src/config/tests/env_tests.rs`, `load_tests.rs`, `loading_tests.rs`
- Modify: `README.md`, `config.example.toml`, `docs/memory.md`

**Interfaces:**
- Retain `Config::load_from(config_path: Option<&Path>) -> anyhow::Result<Config>` and existing path precedence/validation.
- Produce private `parse_config_with(content: &str, lookup: impl FnMut(&str) -> Result<String, VarError>) -> anyhow::Result<Config>`, with production lookup supplied by `std::env::var`. It replaces raw source expansion and unsafe parser chaining.
- Use public `toml::de::DeTable::parse` for recursive `Spanned<DeValue>` nodes, then `Config::deserialize(toml::Deserializer::from(root))`; keep original spans when replacing node contents. Use `DeValue::parse`/`ValueDeserializer` for a single replacement scalar. Avoid `toml::Value::try_into`, which loses typed error spans and imposes an i64 value ceiling absent from the current direct Config deserializer.
- Parsed recursive values retain original spans and string values; a type-specific path table converts only a decoded string that was exactly one placeholder before expansion. Known boolean destinations also accept a whole-value scalar placeholder to preserve existing unquoted boolean compatibility. Exact API/prototype evidence: `.superpowers/sdd/2026-11-15-remediation-configuration/toml-span-notes.md`.

**Scalar destinations:**

| Type | Paths |
| --- | --- |
| i32 | telegram.api_id |
| u32 | rate_limiting.{send_message_cost,send_media_cost,max_tokens,media_download_cost,transcription_cost}; transcription.{default_timeout_seconds,max_timeout_seconds}; logging.max_log_days |
| u64 | telegram.max_download_bytes; telegram.timeouts.{send_secs,upload_send_secs,resolve_secs,history_secs,search_secs,download_secs}; search.deadline_seconds; server.shutdown_timeout_seconds; sending.{max_image_bytes,max_media_bytes,max_staging_bytes}; limits.{response_byte_budget,media_batch_max_total_bytes}; observability.slow_write_threshold_ms; media_artifacts.{ttl_seconds,max_total_bytes} |
| usize | server.{max_request_bytes,max_in_flight_requests}; sending.max_concurrent_media; observability.{response_buffer_size,max_buffered_payload_bytes}; media_artifacts.max_entries |
| f64 | rate_limiting.refill_rate |
| bool | sending.enabled; logging.file_enabled; media_artifacts.{enabled,file_export_enabled} |

- [ ] **Step 1: Add failing value/loader regressions.** Round-trip exact values containing quotes, backslashes, newlines, Unicode, `#`, dollar signs and TOML-looking tables/assignments in basic/literal, single/multiline strings. Assert `${NEXT}` inside a replacement is not recursively looked up. Whole/inline comments and quoted/dotted keys/tables must never look up variables. Test quoted and unquoted whole numeric placeholders at every scalar family, numeric-looking credentials/paths staying strings, and boolean compatibility. Reject scalar injection, arrays/tables, trailing fragments and width/sign errors without changing adjacent fields. Test incomplete placeholder literals, unknown fields and SendingConfig's existing restriction. Missing/non-Unicode and malformed-TOML errors must retain original numeric line/column context and no secret across Display/Debug/source chains. Use injected lookup for pure cases and existing re-executed subprocess fixtures for actual loader/environment behavior; never set/remove parent environment.
- [ ] **Step 2: Run env/loader regression filters.** Expected RED: current source substitution reinterprets replacement contents, expands inline comments and miscoerces digit-only string fields. Replace old implementation-shaped raw-text expectations with actual parsed-value behavior rather than preserving the bug.
- [ ] **Step 3: Implement the lexer → parsed values → typed Config pipeline.** The lexer distinguishes comments, key/value positions, basic/literal and single/multiline quotes/escapes, and masks only legal unquoted whole placeholders (for example `${ID}` → `""   ` at equal width, retaining line breaks). TOML owns general parsing. Walk value strings once and do not visit keys. Parse whole numeric/bool replacements as exactly one appropriate TOML scalar with DeValue::parse, rejecting trailing syntax rather than admitting another document fragment; retain source-node spans and let typed Config deserialization check widths/signs. Do not globally accept arbitrary numeric strings. Map decoded placeholder positions back to original source for multiline errors, including TOML 1.1 escape/folding rules. Drop all unsafe parser/VarError objects and source chains; safe context uses original locations. Retain normal unknown-field and path behavior. Document intentional corrections to accidental comments/keys/raw-string reinterpretation.
- [ ] **Step 4: Run `cargo test --offline --locked config`, full tests, formatting, strict all-target Clippy and warning-free docs.** Expected all pass, including subprocess path precedence and diagnostics isolation. Inspect the scalar table against current Config types once; no public serde field/type changes or new dependency.
- [ ] **Step 5: Commit `fix: interpolate configuration values without TOML injection`; obtain independent task review.**

### Task 2: Reject unusable rate settings (O04)

**Files:**
- Modify: `src/config/validation.rs`, relevant API documentation in `src/config/types.rs`
- Test: `src/config/tests/validation_tests.rs`, `load_tests.rs`, `sending_tests.rs`

**Interfaces:**
- Retain `RateLimitConfig::validate()` and Config loader API; add the finite-positive refill and positive-capacity invariants there.
- Consume Task 1's typed scalar interpolation unchanged; do not touch `RateLimiter::new`.

- [ ] **Step 1: Add direct-validation and loader cases.** Capacity 0 with all costs 0 fails. NaN, +infinity, -infinity, negative/zero refill fail; finite positive refill passes. Zero media/transcription read costs remain accepted; enabled sends still reject zero send costs and all costs remain bounded by capacity.
- [ ] **Step 2: Run new validation/loader filters.** Expected RED for +infinity and zero capacity; NaN/nonpositive cases already pass as counterevidence to the broader finding.
- [ ] **Step 3: Require capacity >=1 and `refill_rate.is_finite() && refill_rate > 0.0` in validation.** Keep existing checks and constructor compatibility.
- [ ] **Step 4: Run config/sending tests and full Rust gate.** Expected all pass with unchanged supported zero read costs.
- [ ] **Step 5: Commit `fix: reject zero capacity and nonfinite token refill`; obtain independent task review.**

## Completion

Record A11/A17 evidence and the partial baseline counterevidence for O04. The maintenance plan removes stale environment-test guidance (O05), integrates Unreleased notes and performs final acceptance.
