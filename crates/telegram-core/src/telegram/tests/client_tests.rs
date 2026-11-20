//! Tests for `TelegramClient` helper fns (`username_to_resolve`).

// `username_to_resolve` is the pure decision the AD-1 consolidation kept from the
// old inline `get_recent_messages` resolver: attempt a username lookup only when
// the identifier (after stripping `@`) is not purely numeric, otherwise fall back
// to the dialog walk by numeric id. One case per prior branch.
mod username_to_resolve_tests {
    use crate::telegram::client::username_to_resolve;

    #[test]
    fn plain_username_is_resolved() {
        assert_eq!(username_to_resolve("durov"), Some("durov"));
    }

    #[test]
    fn at_prefixed_username_is_resolved_with_prefix_kept() {
        // The `@` is stripped later by resolve_username_peer; the returned ref is
        // the raw identifier so the caller's logs match the original.
        assert_eq!(username_to_resolve("@durov"), Some("@durov"));
    }

    #[test]
    fn purely_numeric_identifier_is_not_resolved_as_username() {
        assert_eq!(username_to_resolve("123456"), None);
    }

    #[test]
    fn at_prefixed_numeric_is_not_resolved_as_username() {
        assert_eq!(username_to_resolve("@123456"), None);
    }

    #[test]
    fn empty_or_bare_at_is_not_resolved_as_username() {
        // "@" strips to "" which `chars().all(..)` treats as all-numeric -> None,
        // matching the original guard.
        assert_eq!(username_to_resolve("@"), None);
        assert_eq!(username_to_resolve(""), None);
    }

    #[test]
    fn alphanumeric_username_is_resolved() {
        assert_eq!(username_to_resolve("channel123"), Some("channel123"));
    }
}
