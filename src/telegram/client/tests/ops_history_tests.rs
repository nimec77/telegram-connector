//! Unit tests for the history op's pure decision helpers.

use super::*;

#[test]
fn a_numeric_channel_id_is_the_dialog_walk_target() {
    let id = ChannelId::new(12345).expect("valid channel id");
    assert_eq!(
        dialog_fallback_target(Some(id), None).expect("target"),
        12345
    );
}

#[test]
fn a_username_that_did_not_resolve_hard_errors_instead_of_walking_dialogs() {
    // AD-2: a username reference carries no numeric id, so there is no id to
    // walk dialogs by. Falling back would search for an id we never had.
    let err = dialog_fallback_target(None, Some("@канал"))
        .expect_err("a username with no id must not fall back");
    assert!(
        matches!(err, Error::InvalidInput(ref m) if m.contains("@канал")),
        "the error must name the unresolved reference, got: {err}"
    );
}

#[test]
fn a_missing_id_and_missing_identifier_still_errors_cleanly() {
    let err = dialog_fallback_target(None, None).expect_err("no target is an error");
    assert!(matches!(err, Error::InvalidInput(_)));
}
