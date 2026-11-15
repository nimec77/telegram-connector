use super::*;

fn confirmed_id() -> NonZeroU32 {
    NonZeroU32::new(731).expect("the fixture's confirmation ID is positive")
}

// Catches omitted/duplicate dispatch and changes to accepted text or destination.
#[test]
fn valid_input_dispatches_once_with_verbatim_text() {
    let mut sender = FakeSender::new(Ok(confirmed_id()));
    let result = send_text(
        SendRequest {
            destination: Some(TEST_DESTINATION),
            text: "  Hello **plain text**!\n",
            mode: Mode::Send,
        },
        |destination, text| sender.send(destination, text),
    );

    assert!(matches!(result, Ok(SendOutcome::Sent { .. })));
    assert_eq!(
        sender.attempts,
        [(
            "saved_messages".to_owned(),
            "  Hello **plain text**!\n".to_owned()
        )]
    );
}

// Catches validation after dispatch and treating whitespace as a message.
#[test]
fn empty_or_whitespace_text_never_dispatches() {
    for text in ["", " \t\n", "\u{2003}"] {
        let mut sender = FakeSender::new(Ok(confirmed_id()));
        let result = send_text(
            SendRequest {
                destination: Some(TEST_DESTINATION),
                text,
                mode: Mode::Send,
            },
            |destination, text| sender.send(destination, text),
        );

        assert_eq!(result, Err(ValidationError::EmptyText));
        assert!(sender.attempts.is_empty());
    }
}

// Catches accepting unapproved, ambiguous, multiple or normalized destinations.
#[test]
fn disallowed_or_ambiguous_destination_never_dispatches() {
    for destination in [
        "@someone_else",
        "Alice",
        "123",
        "saved_messages,Alice",
        " saved_messages ",
    ] {
        let mut sender = FakeSender::new(Ok(confirmed_id()));
        let result = send_text(
            SendRequest {
                destination: Some(destination),
                text: "Hello!",
                mode: Mode::Send,
            },
            |destination, text| sender.send(destination, text),
        );

        assert_eq!(result, Err(ValidationError::DisallowedDestination));
        assert!(sender.attempts.is_empty());
    }
}

// Catches fabricating a success ID rather than returning the sender's receipt.
#[test]
fn confirmed_success_returns_senders_message_id() {
    let mut sender = FakeSender::new(Ok(confirmed_id()));
    let result = send_text(
        SendRequest {
            destination: Some(TEST_DESTINATION),
            text: "Hello!",
            mode: Mode::Send,
        },
        |destination, text| sender.send(destination, text),
    );

    assert_eq!(
        result,
        Ok(SendOutcome::Sent {
            message_id: confirmed_id()
        })
    );
    assert_eq!(sender.attempts.len(), 1);
}

// Catches treating lost acknowledgement as failure or silently retrying the send.
#[test]
fn timeout_after_dispatch_is_unknown_without_retry() {
    let mut sender = FakeSender::new(Err(DispatchError::TimeoutAfterDispatch));
    let result = send_text(
        SendRequest {
            destination: Some(TEST_DESTINATION),
            text: "Hello!",
            mode: Mode::Send,
        },
        |destination, text| sender.send(destination, text),
    );

    assert_eq!(result, Ok(SendOutcome::Unknown));
    assert_eq!(sender.attempts.len(), 1);
}

// Catches preview touching the sender or modifying what the user is previewing.
#[test]
fn preview_returns_validated_message_without_dispatch() {
    let mut sender = FakeSender::new(Ok(confirmed_id()));
    let result = send_text(
        SendRequest {
            destination: Some(TEST_DESTINATION),
            text: "  Preview me.\n",
            mode: Mode::Preview,
        },
        |destination, text| sender.send(destination, text),
    );

    assert_eq!(
        result,
        Ok(SendOutcome::Preview {
            destination: "saved_messages".to_owned(),
            text: "  Preview me.\n".to_owned(),
        })
    );
    assert!(sender.attempts.is_empty());
}

// Catches silently choosing a default destination when the caller omits it.
#[test]
fn missing_destination_never_dispatches() {
    for destination in [None, Some(""), Some(" \t\n")] {
        let mut sender = FakeSender::new(Ok(confirmed_id()));
        let result = send_text(
            SendRequest {
                destination,
                text: "Hello!",
                mode: Mode::Send,
            },
            |destination, text| sender.send(destination, text),
        );

        assert_eq!(result, Err(ValidationError::MissingDestination));
        assert!(sender.attempts.is_empty());
    }
}

// Catches collapsing a definite refusal into an unknown delivery, or retrying it.
#[test]
fn definitive_refusal_is_not_sent_without_retry() {
    let mut sender = FakeSender::new(Err(DispatchError::Rejected));
    let result = send_text(
        SendRequest {
            destination: Some(TEST_DESTINATION),
            text: "Hello!",
            mode: Mode::Send,
        },
        |destination, text| sender.send(destination, text),
    );

    assert_eq!(result, Ok(SendOutcome::NotSent));
    assert_eq!(sender.attempts.len(), 1);
}

// Catches preview bypassing validation and suggesting an invalid send is ready.
#[test]
fn invalid_preview_is_rejected_without_dispatch() {
    for (destination, text, expected) in [
        (Some(TEST_DESTINATION), "", ValidationError::EmptyText),
        (None, "Hello!", ValidationError::MissingDestination),
        (
            Some("Alice"),
            "Hello!",
            ValidationError::DisallowedDestination,
        ),
    ] {
        let mut sender = FakeSender::new(Ok(confirmed_id()));
        let result = send_text(
            SendRequest {
                destination,
                text,
                mode: Mode::Preview,
            },
            |destination, text| sender.send(destination, text),
        );

        assert_eq!(result, Err(expected));
        assert!(sender.attempts.is_empty());
    }
}
