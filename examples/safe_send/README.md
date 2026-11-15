# The smallest safe send — offline Rust example

Runnable code for [Telegram Learning Lab, lesson 01](https://nimec77.github.io/telegram-learning-lab/#contract).
Every send stays inside an in-memory fake. Running this example requires no Telegram
credentials, authenticated session, or account configuration.

From the repository root, with the pinned toolchain and dependencies cached:

```sh
cargo run --offline --locked --example safe_send
cargo test --offline --locked --example safe_send
```

The demo prints each result and its fake sender call count to stderr. The five lesson
cases appear first, followed by preview, missing destination, and a definitive refusal.
The tests also run as part of normal `cargo test --offline --locked`.

## Read the code

- `main.rs`: private request/result types, `send_text`, the recording `FakeSender`,
  and a runnable demonstration.
- `tests.rs`: the five lesson cases and additional safety checks.

`send_text` accepts an injected closure as its dispatch boundary. The fake records
the destination and text, then supplies a scripted confirmation or failure. The
timeout case returns immediately: it simulates a lost acknowledgement without
waiting or contacting Telegram.

## The contract

1. Each request supplies one explicit destination and plain text. The only allowed
   destination is the exact literal `saved_messages`, representing the chosen test
   destination. Missing/blank values, names, usernames, bare IDs, multiple destinations,
   and surrounding destination whitespace are rejected without resolution.
2. Empty or whitespace-only text is rejected. Accepted text is preserved verbatim;
   Markdown/HTML are ordinary characters. There is no media, fan-out or scheduling.
3. Validation finishes before the dispatch closure can run. Invalid input causes
   zero sender calls, including in preview mode.
4. Preview returns the validated destination and text with zero sender calls.
5. A valid send invokes the closure exactly once. Confirmed success returns its
   positive synthetic message ID; the example does not invent an ID in `send_text`.
6. A definitive refusal yields `NotSent`. A timeout after dispatch yields `Unknown`,
   because the message might have been accepted. Neither outcome triggers a retry.

Validation failures use `Err(ValidationError::...)`. Once validation succeeds, the
result is `Ok(SendOutcome::...)`; callers must inspect the outcome to determine
whether sending was confirmed. `Ok(Unknown)` is not confirmation of delivery.

## Five required tests

| Lesson case | Test | Expected sender calls |
| --- | --- | --- |
| Valid input | `valid_input_dispatches_once_with_verbatim_text` | 1, with exact input |
| Empty text | `empty_or_whitespace_text_never_dispatches` | 0 |
| Disallowed destination | `disallowed_or_ambiguous_destination_never_dispatches` | 0 |
| Confirmed success | `confirmed_success_returns_senders_message_id` | 1, returning ID 731 |
| Timeout after dispatch | `timeout_after_dispatch_is_unknown_without_retry` | 1, returning `Unknown` |

Additional tests cover preview, invalid preview, missing destinations, and a definitive
refusal. Unicode whitespace and unchanged plain text are included.

## Connecting the lesson to the real project

This example models the decision boundary only. Its synchronous fake does not prove
Telegram delivery, implement a real deadline, or provide exactly-once delivery.
Manually repeating an unknown send can still create a duplicate. A successful preview
does not authorize a later send; each invocation is validated independently.

The project's existing opt-in sending implementation has its own recipient types,
limits, timeouts and receipts; see [the production sending guide](../../docs/sending.md).
The exercise adds no MCP tools and makes no changes to `TelegramClientTrait`.
