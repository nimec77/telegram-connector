# Offline safe-send learning example

Implement the learning lab's “The smallest safe send” contract as a runnable Rust
example with an in-memory fake sender. The user requested code in a dedicated folder
and explicitly prohibited commits and pushes.

## Approach

Use Cargo's existing `examples/` convention: `examples/safe_send/main.rs`, sibling
`tests.rs`, and a README containing the contract and commands. A new top-level
`example/` folder would need extra Cargo wiring; modifying the real MCP sender would
expand the exercise beyond its offline scope. These are implementation choices.
Register the example with `test = true` in `Cargo.toml` so normal `cargo test` runs it.

## Contract

- Exactly one allowed destination: the literal `saved_messages`, with no resolution.
- Reject missing/blank destinations, all other destination strings, and blank text.
- Preserve accepted text verbatim; preview validates but never dispatches.
- A valid send calls the injected sender exactly once.
- Confirmed success returns the sender's positive synthetic message ID.
- A definitive refusal returns `NotSent`; a timeout after dispatch returns `Unknown`.
- No automatic retries. A caller manually repeating an unknown send can duplicate it.
- Fake outcomes are synchronous simulations, including the timeout case.
- No Telegram session, authentication, network, new dependencies, or production changes.

## Structure and validation

Keep private request/result/error types, the dispatch function and a small recording
fake in the example. Inject a closure rather than extend `TelegramClientTrait`.
The README maps the five lesson cases to tests and explains the additional guards.

Verify failing behavior tests before implementation, then the runnable example,
explicit example tests, formatting, all-target Clippy, the full offline test suite,
and warning-free rustdoc on the repository's pinned nightly/macOS environment.
Leave all work uncommitted and unpushed.
