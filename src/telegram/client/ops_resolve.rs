//! Batch identifier -> channel resolution (work-order A7).
//!
//! Unit of `client` (LM-2). One dialog walk serves ids, subscribed usernames,
//! and titles; only unmatched username-shaped identifiers spend a
//! `resolve_username` RPC.

use super::*;
use crate::telegram::types::{Channel, ChannelResolution};

/// Verdict of matching one identifier against the subscribed dialog list.
#[derive(Debug)]
pub(super) enum MatchOutcome {
    Found(Channel),
    /// Valid username shape, not among subscribed dialogs — worth one RPC.
    TryUsernameRpc,
    /// More than one subscribed chat carries this title (count attached).
    Ambiguous(usize),
    NotFound,
}

pub(super) fn match_identifier(identifier: &str, dialogs: &[Channel]) -> MatchOutcome {
    let trimmed = identifier.trim();
    if let Ok(id) = trimmed.parse::<i64>() {
        return match dialogs.iter().find(|c| c.id.get() == id) {
            Some(c) => MatchOutcome::Found(c.clone()),
            None => MatchOutcome::NotFound,
        };
    }
    let bare = trimmed.strip_prefix('@').unwrap_or(trimmed);
    if Username::is_valid_shape(bare) {
        if let Some(c) = dialogs.iter().find(|c| {
            c.username
                .as_ref()
                .is_some_and(|u| u.as_str().eq_ignore_ascii_case(bare))
        }) {
            return MatchOutcome::Found(c.clone());
        }
        return MatchOutcome::TryUsernameRpc;
    }
    // Title path: exact, trimmed, case-insensitive (Unicode-aware lowercase).
    let wanted = trimmed.to_lowercase();
    let mut matches = dialogs
        .iter()
        .filter(|c| c.name.as_str().trim().to_lowercase() == wanted);
    match (matches.next(), matches.count()) {
        (Some(c), 0) => MatchOutcome::Found(c.clone()),
        (Some(_), extra) => MatchOutcome::Ambiguous(extra + 1),
        (None, _) => MatchOutcome::NotFound,
    }
}

impl TelegramClient {
    pub(super) async fn resolve_channels_impl(
        &self,
        identifiers: &[String],
    ) -> Result<Vec<ChannelResolution>, Error> {
        // One walk over the whole dialog list (same shape as
        // get_subscribed_channels_impl, without pagination).
        let dialogs = with_timeout("iter_dialogs", self.timeouts.resolve_secs, async {
            let mut collected = Vec::new();
            let mut iter = self.client.iter_dialogs();
            while let Some(dialog) = iter.next().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to iterate dialogs in resolve_channels");
                Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
            })? {
                if let Some(channel) = convert_peer_to_channel(dialog.peer()) {
                    collected.push(channel);
                }
            }
            Ok(collected)
        })
        .await?;

        let mut out = Vec::with_capacity(identifiers.len());
        for identifier in identifiers {
            let resolution = match match_identifier(identifier, &dialogs) {
                MatchOutcome::Found(channel) => ChannelResolution {
                    identifier: identifier.clone(),
                    channel: Some(channel),
                    error: None,
                },
                MatchOutcome::Ambiguous(n) => ChannelResolution {
                    identifier: identifier.clone(),
                    channel: None,
                    error: Some(format!(
                        "ambiguous title: {n} subscribed chats are named '{}'",
                        identifier.trim()
                    )),
                },
                MatchOutcome::TryUsernameRpc => {
                    match self.resolve_username_peer(identifier.trim()).await {
                        Ok(Some(peer)) => match convert_peer_to_channel(&peer) {
                            Some(channel) => ChannelResolution {
                                identifier: identifier.clone(),
                                channel: Some(channel),
                                error: None,
                            },
                            None => ChannelResolution {
                                identifier: identifier.clone(),
                                channel: None,
                                error: Some("Not a channel or group".to_string()),
                            },
                        },
                        Ok(None) => not_found(identifier),
                        // Per-identifier RPC failure degrades to that entry,
                        // not the whole batch.
                        Err(e) => ChannelResolution {
                            identifier: identifier.clone(),
                            channel: None,
                            error: Some(e.to_string()),
                        },
                    }
                }
                MatchOutcome::NotFound => not_found(identifier),
            };
            out.push(resolution);
        }
        Ok(out)
    }
}

fn not_found(identifier: &str) -> ChannelResolution {
    ChannelResolution {
        identifier: identifier.to_string(),
        channel: None,
        error: Some(format!("Channel not found: {}", identifier.trim())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::{ChannelName, Username};
    use crate::test_helpers::create_test_channel;

    fn dialogs() -> Vec<Channel> {
        let mut family = create_test_channel(521440428, "family");
        family.name = ChannelName::new("Семейный чатик").expect("name");
        family.username = None;
        let mut swodki = create_test_channel(1144180066, "swodki");
        swodki.name = ChannelName::new("Сводки").expect("name");
        swodki.username = Some(Username::new("swodki").expect("username"));
        vec![family, swodki]
    }

    #[test]
    fn numeric_identifier_matches_dialog_by_id() {
        match match_identifier("521440428", &dialogs()) {
            MatchOutcome::Found(c) => assert_eq!(c.id.get(), 521440428),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn username_identifier_matches_dialog_case_insensitively() {
        match match_identifier("@SwOdKi", &dialogs()) {
            MatchOutcome::Found(c) => assert_eq!(c.id.get(), 1144180066),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn unmatched_username_shape_defers_to_rpc() {
        assert!(matches!(
            match_identifier("not_subscribed", &dialogs()),
            MatchOutcome::TryUsernameRpc
        ));
    }

    #[test]
    fn title_identifier_matches_exactly_trimmed_case_insensitive() {
        match match_identifier("  семейный чатик ", &dialogs()) {
            MatchOutcome::Found(c) => assert_eq!(c.id.get(), 521440428),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn unknown_title_and_unknown_id_are_not_found() {
        assert!(matches!(
            match_identifier("Нет такого чата!", &dialogs()),
            MatchOutcome::NotFound
        ));
        assert!(matches!(
            match_identifier("999999", &dialogs()),
            MatchOutcome::NotFound
        ));
    }

    #[test]
    fn duplicate_titles_are_ambiguous() {
        let mut d = dialogs();
        let mut dup = create_test_channel(777, "dupchannel");
        dup.name = ChannelName::new("Сводки").expect("name");
        dup.username = None;
        d.push(dup);
        // "Сводки" is both a title of two chats — never guess.
        assert!(matches!(
            match_identifier("сводки б/у titles only", &d),
            MatchOutcome::NotFound
        ));
        assert!(matches!(
            match_identifier("Сводки", &d),
            MatchOutcome::Ambiguous(2)
        ));
    }
}
