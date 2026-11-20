//! Offline exercise for “The smallest safe send” in Telegram Learning Lab.
//! Run: cargo run --offline --locked --example safe_send
//! Test: cargo test --offline --locked --example safe_send

use std::num::NonZeroU32;

#[cfg(test)]
mod tests;

const TEST_DESTINATION: &str = "saved_messages";

#[derive(Clone, Copy, Debug)]
enum Mode {
    Preview,
    Send,
}

struct SendRequest<'a> {
    destination: Option<&'a str>,
    text: &'a str,
    mode: Mode,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum ValidationError {
    #[error("text must not be empty or whitespace only")]
    EmptyText,
    #[error("an explicit destination is required")]
    MissingDestination,
    #[error("only the exact test destination is allowed")]
    DisallowedDestination,
}

#[derive(Clone, Copy, Debug)]
enum DispatchError {
    Rejected,
    TimeoutAfterDispatch,
}

#[derive(Debug, PartialEq, Eq)]
enum SendOutcome {
    Preview { destination: String, text: String },
    Sent { message_id: NonZeroU32 },
    NotSent,
    Unknown,
}

/// Validate locally, preview without dispatch, or make one send attempt.
///
/// The injected boundary supplies a confirmed positive ID or a classified error.
/// An unknown result must never trigger an automatic second attempt.
///
/// # Errors
/// Returns a validation error without invoking the sender for invalid inputs.
fn send_text(
    request: SendRequest<'_>,
    mut dispatch: impl FnMut(&str, &str) -> Result<NonZeroU32, DispatchError>,
) -> Result<SendOutcome, ValidationError> {
    if request.text.trim().is_empty() {
        return Err(ValidationError::EmptyText);
    }

    let destination = request
        .destination
        .filter(|value| !value.trim().is_empty())
        .ok_or(ValidationError::MissingDestination)?;
    if destination != TEST_DESTINATION {
        return Err(ValidationError::DisallowedDestination);
    }

    if matches!(request.mode, Mode::Preview) {
        return Ok(SendOutcome::Preview {
            destination: destination.to_owned(),
            text: request.text.to_owned(),
        });
    }

    // One attempt only: losing confirmation does not prove the send failed.
    Ok(match dispatch(destination, request.text) {
        Ok(message_id) => SendOutcome::Sent { message_id },
        Err(DispatchError::Rejected) => SendOutcome::NotSent,
        Err(DispatchError::TimeoutAfterDispatch) => SendOutcome::Unknown,
    })
}

/// Records attempts in memory; this type has no Telegram or network access.
struct FakeSender {
    result: Result<NonZeroU32, DispatchError>,
    attempts: Vec<(String, String)>,
}

impl FakeSender {
    fn new(result: Result<NonZeroU32, DispatchError>) -> Self {
        Self {
            result,
            attempts: Vec::new(),
        }
    }

    fn send(&mut self, destination: &str, text: &str) -> Result<NonZeroU32, DispatchError> {
        self.attempts
            .push((destination.to_owned(), text.to_owned()));
        self.result
    }
}

fn main() {
    let confirmed_id = NonZeroU32::new(42).expect("the demo's synthetic message ID is positive");
    let cases = [
        (
            "Valid input",
            Some(TEST_DESTINATION),
            "Hello!",
            Mode::Send,
            Ok(confirmed_id),
        ),
        (
            "Empty text",
            Some(TEST_DESTINATION),
            "",
            Mode::Send,
            Ok(confirmed_id),
        ),
        (
            "Disallowed destination",
            Some("Alice"),
            "Hello!",
            Mode::Send,
            Ok(confirmed_id),
        ),
        (
            "Confirmed success",
            Some(TEST_DESTINATION),
            "Hello!",
            Mode::Send,
            Ok(confirmed_id),
        ),
        (
            "Timeout after dispatch",
            Some(TEST_DESTINATION),
            "Hello!",
            Mode::Send,
            Err(DispatchError::TimeoutAfterDispatch),
        ),
        (
            "Preview",
            Some(TEST_DESTINATION),
            "Hello!",
            Mode::Preview,
            Ok(confirmed_id),
        ),
        (
            "Missing destination",
            None,
            "Hello!",
            Mode::Send,
            Ok(confirmed_id),
        ),
        (
            "Definitive refusal",
            Some(TEST_DESTINATION),
            "Hello!",
            Mode::Send,
            Err(DispatchError::Rejected),
        ),
    ];

    eprintln!("Offline demonstration: all dispatches stay inside an in-memory fake sender.");
    for (label, destination, text, mode, result) in cases {
        let mut sender = FakeSender::new(result);
        let outcome = send_text(
            SendRequest {
                destination,
                text,
                mode,
            },
            |destination, text| sender.send(destination, text),
        );
        eprintln!(
            "{label}: {outcome:?}; fake sender calls: {}",
            sender.attempts.len()
        );
    }
}
