# Offline Safe Send Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Supply the lesson's safe-send contract as runnable offline Rust code.

**Architecture:** One Cargo example with private types, a closure-injected dispatch
boundary and a recording fake. Sibling tests exercise validation, preview and delivery
outcomes. A README explains the simplified contract.

**Tech Stack:** Rust 2024, pinned nightly, existing thiserror dependency.

**Spec:** `docs/superpowers/specs/2026-11-15-safe-send-example-design.md`

## Global Constraints

- Exactly one allowed destination: the literal `saved_messages`, with no resolution.
- No Telegram session, authentication, network, new dependencies, or production changes.
- No automatic retries.
- Leave all work uncommitted and unpushed.

## Review Focus

- Blank/whitespace text must fail before dispatch.
- Missing/blank destinations must fail before dispatch.
- Ambiguous destination strings must not be resolved or accepted.
- Preview must validate while making zero sender calls.
- Timeout after dispatch must remain unknown with exactly one attempt.

### Task 1: Runnable contract, fake sender, tests and guide

**Files:** Create `examples/safe_send/main.rs`, `examples/safe_send/tests.rs`,
and `examples/safe_send/README.md`. Modify `Cargo.toml` to register the example with
`test = true` so its tests run with the normal suite.

**Interfaces:** Private `SendRequest<'a>` carries `destination: Option<&'a str>`,
`text: &'a str`, and `mode: Mode`. `send_text(request, dispatch)` accepts
`impl FnMut(&str, &str) -> Result<NonZeroU32, DispatchError>` and returns
`Result<SendOutcome, ValidationError>`. The fake records `(String, String)` attempts.

- [x] Write tests for the five lesson cases and the five Review Focus conditions;
  additionally check preserved whitespace/plain text and a definitive refusal.
- [x] Run `cargo test --offline --locked --example safe_send`; expect behavior
  tests to fail against a compiling unimplemented dispatch function.
- [x] Implement validation, side-effect-free preview, exactly one dispatch, and
  typed outcome mapping. Add a demo of the scripted cases and README run commands.
- [x] Run the example tests and `cargo run --offline --locked --example safe_send`;
  expect all tests to pass and printed attempt counts of zero or one as appropriate.
- [x] Run the full repository gate with `--offline --locked`, inspect the scoped
  changes and obtain independent review. Leave files unstaged; do not commit/push.

Validation: 1,188 library tests passed (12 existing ignored), 9 example tests passed,
3 doctests passed; formatting, strict all-target Clippy, warning-free rustdoc and
the runnable demo passed. Independent read-only review approved the uncommitted
deliverable with no findings.
