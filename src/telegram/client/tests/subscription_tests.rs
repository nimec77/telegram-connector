//! Raw username observations, routing and cache effects below the whole-client mock.
use super::*;
use crate::telegram::converters::{convert_discovered_peer, convert_peer_to_channel};
use crate::test_helpers::{raw_tl_channel, raw_tl_community, raw_tl_user};
use std::cell::Cell;

fn channel_peer_id(id: i64) -> tl::enums::Peer {
    tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: id })
}

fn envelope(chat: tl::enums::Chat) -> tl::types::contacts::ResolvedPeer {
    let peer = match &chat {
        tl::enums::Chat::Empty(c) => tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: c.id }),
        tl::enums::Chat::Chat(c) => tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: c.id }),
        tl::enums::Chat::Forbidden(c) => {
            tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: c.id })
        }
        tl::enums::Chat::Channel(c) => channel_peer_id(c.id),
        tl::enums::Chat::ChannelForbidden(c) => channel_peer_id(c.id),
        tl::enums::Chat::Community(c) => channel_peer_id(c.id),
        tl::enums::Chat::CommunityForbidden(c) => channel_peer_id(c.id),
    };
    tl::types::contacts::ResolvedPeer {
        peer,
        users: vec![],
        chats: vec![chat],
    }
}

fn observations() -> Vec<(&'static str, tl::enums::Chat, bool)> {
    let mut cases = vec![
        (
            "empty",
            tl::enums::Chat::Empty(tl::types::ChatEmpty { id: 7 }),
            false,
        ),
        (
            "forbidden chat",
            tl::enums::Chat::Forbidden(tl::types::ChatForbidden {
                id: 7,
                title: "Forbidden".into(),
            }),
            false,
        ),
    ];
    for (name, left, deactivated, creator, expected) in [
        ("active chat", false, false, false, true),
        ("left chat", true, false, false, false),
        ("deactivated creator chat", false, true, true, false),
    ] {
        cases.push((
            name,
            tl::enums::Chat::Chat(tl::types::Chat {
                creator,
                left,
                deactivated,
                call_active: false,
                call_not_empty: false,
                noforwards: false,
                id: 7,
                title: "Small group".into(),
                photo: tl::enums::ChatPhoto::Empty,
                participants_count: 3,
                date: 0,
                version: 1,
                migrated_to: None,
                admin_rights: None,
                default_banned_rights: None,
            }),
            expected,
        ));
    }
    for broadcast in [true, false] {
        for (name, left, min, creator, expected) in [
            ("joined channel", false, false, false, true),
            ("left channel", true, false, false, false),
            ("minimal channel", false, true, false, false),
            ("minimal creator channel", false, true, true, false),
            ("left creator channel", true, true, true, false),
        ] {
            let mut chat = raw_tl_channel(7, "Channel", Some("channelname"));
            chat.broadcast = broadcast;
            chat.megagroup = !broadcast;
            chat.left = left;
            chat.min = min;
            chat.creator = creator;
            cases.push((name, tl::enums::Chat::Channel(chat), expected));
        }
        cases.push((
            "forbidden channel",
            tl::enums::Chat::ChannelForbidden(tl::types::ChannelForbidden {
                broadcast,
                megagroup: !broadcast,
                monoforum: false,
                id: 7,
                access_hash: 9,
                title: "Forbidden".into(),
                until_date: None,
            }),
            false,
        ));
    }
    for (name, left, min, creator, expected) in [
        ("joined community", false, false, false, true),
        ("left community", true, false, false, false),
        ("minimal community", false, true, false, false),
        ("minimal creator community", false, true, true, false),
        ("left creator community", true, true, true, false),
    ] {
        let mut chat = raw_tl_community(7, "Community");
        chat.left = left;
        chat.min = min;
        chat.creator = creator;
        cases.push((name, tl::enums::Chat::Community(chat), expected));
    }
    for hash in [None, Some(9)] {
        cases.push((
            "forbidden community",
            tl::enums::Chat::CommunityForbidden(tl::types::CommunityForbidden {
                id: 7,
                access_hash: hash,
                title: "Forbidden".into(),
            }),
            false,
        ));
    }
    let mut no_hash = raw_tl_channel(7, "No hash", None);
    no_hash.access_hash = None;
    cases.push((
        "full joined without hash",
        tl::enums::Chat::Channel(no_hash),
        true,
    ));
    cases
}

#[tokio::test]
async fn username_subscription_raw_variants_preserve_membership_before_normalization() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let mut mismatches = Vec::new();
    for (label, raw, expected) in observations() {
        let (resolved, _) = decode_resolved_peer(&client.client, envelope(raw));
        let resolved = resolved.expect("selected raw peer");
        let channel = resolved.channel().expect("channel-like peer");
        if channel.is_subscribed != expected {
            mismatches.push(label);
        }
        assert!(
            convert_peer_to_channel(&resolved.peer)
                .expect("dialog context")
                .is_subscribed
        );
        assert!(
            !convert_discovered_peer(&resolved.peer)
                .expect("public discovery")
                .is_subscribed
        );
    }
    assert!(
        mismatches.is_empty(),
        "false subscription claims: {mismatches:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn username_subscription_basic_full_and_unmatched_batch_use_raw_evidence() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    for (label, raw, expected) in observations() {
        let rpc_calls = Cell::new(0);
        let caches = Cell::new(0);
        let response = envelope(raw.clone());
        let resolved = resolve_channel_with(
            "@channelname",
            async { panic!("username must not walk dialogs") },
            resolve_username_with(
                &client.client,
                "@channelname",
                2,
                async {
                    rpc_calls.set(rpc_calls.get() + 1);
                    Ok(tl::enums::contacts::ResolvedPeer::Peer(response))
                },
                |peers| async {
                    caches.set(caches.get() + 1);
                    client.session.cache_peers(peers).await
                },
            ),
        )
        .await
        .expect("username route");
        let basic_subscribed = resolved.channel().expect("basic").is_subscribed;
        let full = client
            .full_channel_from_resolved(resolved)
            .await
            .expect("failed enrichment retains basic");

        assert!(full.description.is_none());
        assert!(full.last_message_date.is_none());
        assert_eq!(rpc_calls.get(), 1, "one username RPC, no membership RPC");
        assert_eq!(caches.get(), 1);
        let out = client
            .resolve_channels_with(&["channelname".into()], &[], |reference| {
                let response = envelope(raw.clone());
                let client = &client;
                async move {
                    resolve_username_with(
                        &client.client,
                        &reference,
                        2,
                        async { Ok(tl::enums::contacts::ResolvedPeer::Peer(response)) },
                        |peers| client.session.cache_peers(peers),
                    )
                    .await
                }
            })
            .await
            .expect("batch");
        assert_eq!(
            [
                basic_subscribed,
                full.is_subscribed,
                out[0]
                    .channel
                    .as_ref()
                    .expect("resolved batch item")
                    .is_subscribed
            ],
            [expected; 3],
            "basic/full/batch {label}"
        );
        assert!(out[0].error.is_none());
    }
}

#[tokio::test]
async fn subscription_dialog_route_does_not_resolve_username() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let mut raw = raw_tl_channel(7, "Partial dialog", None);
    raw.min = true;
    let peer = Peer::from_raw(&client.client, tl::enums::Chat::Channel(raw));
    let resolved = resolve_channel_with("7", async { Ok(Some(peer)) }, async {
        panic!("numeric route must not resolve username")
    })
    .await
    .expect("dialog");
    assert!(resolved.channel().expect("channel").is_subscribed);
}

#[tokio::test]
async fn username_subscription_cache_preserves_entire_typed_envelope_and_last_duplicate() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let mut first = raw_tl_channel(7, "first", None);
    first.access_hash = Some(11);
    let mut last = first.clone();
    last.title = "last".into();
    last.left = true;
    last.access_hash = Some(22);
    let mut minimal = raw_tl_channel(8, "minimal", None);
    minimal.min = true;
    let mut missing_hash = raw_tl_channel(9, "missing hash", None);
    missing_hash.access_hash = None;
    let mut user = raw_tl_user(7, Some("User"), None, None);
    user.access_hash = Some(33);
    let mut minimal_user = raw_tl_user(8, None, None, None);
    minimal_user.min = true;
    minimal_user.access_hash = Some(99);
    let response = tl::types::contacts::ResolvedPeer {
        peer: channel_peer_id(7),
        users: vec![
            tl::enums::User::User(user),
            tl::enums::User::User(minimal_user),
            tl::enums::User::Empty(tl::types::UserEmpty { id: 9 }),
        ],
        chats: vec![
            tl::enums::Chat::Channel(first),
            tl::enums::Chat::Empty(tl::types::ChatEmpty { id: 7 }),
            tl::enums::Chat::Channel(last),
            tl::enums::Chat::Channel(minimal),
            tl::enums::Chat::Channel(missing_hash),
        ],
    };
    let cache_calls = Cell::new(0);
    let resolved = resolve_username_with(
        &client.client,
        "channelname",
        2,
        async { Ok(tl::enums::contacts::ResolvedPeer::Peer(response)) },
        |infos| {
            cache_calls.set(cache_calls.get() + 1);
            assert_eq!(
                infos.len(),
                3,
                "user/chat/channel same bare ID remain distinct; skip unauthorized"
            );
            client.session.cache_peers(infos)
        },
    )
    .await
    .expect("resolve")
    .expect("selected");
    assert_eq!(resolved.channel().expect("channel").name.as_str(), "last");
    assert!(!resolved.channel().expect("channel").is_subscribed);
    assert_eq!(cache_calls.get(), 1);
    for peer in [
        channel_peer_id(7),
        tl::enums::Peer::Chat(tl::types::PeerChat { chat_id: 7 }),
        tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 }),
    ] {
        assert!(
            client
                .session
                .peer_ref(PeerId::from(peer))
                .await
                .expect("session")
                .is_some()
        );
    }
    for peer in [
        channel_peer_id(8),
        channel_peer_id(9),
        tl::enums::Peer::User(tl::types::PeerUser { user_id: 8 }),
        tl::enums::Peer::User(tl::types::PeerUser { user_id: 9 }),
    ] {
        assert!(
            client
                .session
                .peer(PeerId::from(peer))
                .await
                .expect("session")
                .is_none()
        );
    }
    let prior = client
        .session
        .peer(PeerId::from(channel_peer_id(7)))
        .await
        .expect("session")
        .expect("authorized");
    assert_eq!(
        prior.auth().expect("auth").hash(),
        22,
        "last duplicate cache authority"
    );
    let mut minimal = raw_tl_channel(7, "minimal later", None);
    minimal.min = true;
    minimal.access_hash = Some(44);
    let result = resolve_username_with(
        &client.client,
        "channelname",
        2,
        async {
            Ok(tl::enums::contacts::ResolvedPeer::Peer(envelope(
                tl::enums::Chat::Channel(minimal),
            )))
        },
        |infos| client.session.cache_peers(infos),
    )
    .await
    .expect("minimal")
    .expect("present");
    assert!(!result.channel().expect("minimal channel").is_subscribed);
    let after = client
        .session
        .peer(PeerId::from(channel_peer_id(7)))
        .await
        .expect("session")
        .expect("still authorized");
    assert_eq!(
        prior.auth(),
        after.auth(),
        "minimal observation must not replace usable authority"
    );
}

#[tokio::test]
async fn username_subscription_missing_selection_still_caches_authorized_envelope() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let mut response = envelope(tl::enums::Chat::Channel(raw_tl_channel(
        7,
        "Unselected",
        None,
    )));
    response.peer = channel_peer_id(99);
    let result = resolve_username_with(
        &client.client,
        "channelname",
        2,
        async { Ok(tl::enums::contacts::ResolvedPeer::Peer(response)) },
        |infos| client.session.cache_peers(infos),
    )
    .await
    .expect("missing");
    assert!(result.is_none());
    assert!(
        client
            .session
            .peer_ref(PeerId::from(channel_peer_id(7)))
            .await
            .expect("session")
            .is_some()
    );
}

#[derive(Clone)]
struct DiagnosticCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for DiagnosticCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn username_subscription_cache_failure_is_best_effort() {
    use tracing::instrument::WithSubscriber as _;
    let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let writer = DiagnosticCapture(std::sync::Arc::clone(&output));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let response = envelope(tl::enums::Chat::Channel(raw_tl_channel(7, "Joined", None)));
    let resolved = resolve_username_with(
        &client.client,
        "channelname",
        2,
        async { Ok(tl::enums::contacts::ResolvedPeer::Peer(response)) },
        |_| async { Err("PRIVATE_DATABASE_PATH") },
    )
    .with_subscriber(subscriber)
    .await
    .expect("best effort")
    .expect("peer");
    assert!(resolved.channel().expect("joined").is_subscribed);
    let bytes = output.lock().expect("captured diagnostic");
    let diagnostic = String::from_utf8_lossy(&bytes);
    assert!(diagnostic.contains("Could not cache resolved username peers"));
    assert!(!diagnostic.contains("PRIVATE_DATABASE_PATH"));
}

#[tokio::test(start_paused = true)]
async fn username_subscription_cache_wait_uses_original_resolve_deadline() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let response = envelope(tl::enums::Chat::Channel(raw_tl_channel(7, "Joined", None)));
    let start = tokio::time::Instant::now();
    let result = resolve_username_with(
        &client.client,
        "channelname",
        2,
        async {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            Ok(tl::enums::contacts::ResolvedPeer::Peer(response))
        },
        |_| std::future::pending::<Result<(), ()>>(),
    )
    .await;
    assert!(matches!(result, Err(Error::Timeout { .. })));
    assert_eq!(start.elapsed(), std::time::Duration::from_secs(2));
}

#[tokio::test]
async fn username_subscription_absence_validation_and_errors_do_not_cache() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let invalid = resolve_username_with(
        &client.client,
        "bad name",
        2,
        async { panic!("invalid username must not invoke") },
        |_| -> std::future::Ready<Result<(), ()>> { panic!("no response must not cache") },
    )
    .await
    .expect("local absence");
    assert!(invalid.is_none());
    for name in ["USERNAME_NOT_OCCUPIED", "SECRET_PROVIDER_ERROR"] {
        let result = resolve_username_with(
            &client.client,
            "channelname",
            2,
            async {
                Err(InvocationError::Rpc(grammers_mtsender::RpcError {
                    code: 400,
                    name: name.into(),
                    value: None,
                    caused_by: None,
                }))
            },
            |_| -> std::future::Ready<Result<(), ()>> { panic!("RPC error must not cache") },
        )
        .await;
        if name == "USERNAME_NOT_OCCUPIED" {
            assert!(result.expect("missing username").is_none());
        } else {
            let Err(error) = result else {
                panic!("RPC failure must survive")
            };
            assert!(matches!(error, Error::TelegramApi(_)));
            assert!(!format!("{error:?} {error}").contains(name));
        }
    }
}

#[tokio::test]
async fn username_subscription_forbidden_cache_authority_is_independent_of_membership() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    for broadcast in [true, false] {
        let raw = tl::enums::Chat::ChannelForbidden(tl::types::ChannelForbidden {
            broadcast,
            megagroup: !broadcast,
            monoforum: false,
            id: 7,
            access_hash: 91,
            title: "Forbidden".into(),
            until_date: None,
        });
        let (resolved, cache) = decode_resolved_peer(&client.client, envelope(raw));
        assert!(
            !resolved
                .expect("peer")
                .channel()
                .expect("channel")
                .is_subscribed
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache[0].auth().expect("forbidden peer still usable").hash(),
            91
        );
    }
    for hash in [None, Some(92)] {
        let raw = tl::enums::Chat::CommunityForbidden(tl::types::CommunityForbidden {
            id: 7,
            access_hash: hash,
            title: "Forbidden".into(),
        });
        let (resolved, cache) = decode_resolved_peer(&client.client, envelope(raw));
        assert!(
            !resolved
                .expect("peer")
                .channel()
                .expect("community")
                .is_subscribed
        );
        assert_eq!(cache.len(), usize::from(hash.is_some()));
    }
    let mut minimal = raw_tl_community(7, "minimal");
    minimal.min = true;
    let (_, cache) = decode_resolved_peer(
        &client.client,
        envelope(tl::enums::Chat::Community(minimal)),
    );
    assert!(cache.is_empty());
}

#[tokio::test]
async fn username_subscription_final_minimal_duplicate_is_not_cached() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let joined = raw_tl_channel(7, "joined first", None);
    let mut minimal = joined.clone();
    minimal.min = true;
    minimal.title = "minimal last".into();
    let mut response = envelope(tl::enums::Chat::Channel(joined));
    response.chats.push(tl::enums::Chat::Channel(minimal));
    let (resolved, cache) = decode_resolved_peer(&client.client, response);
    assert!(
        cache.is_empty(),
        "last duplicate wins before the authority filter"
    );
    let channel = resolved.expect("peer").channel().expect("channel");
    assert_eq!(channel.name.as_str(), "minimal last");
    assert!(!channel.is_subscribed);
}

#[tokio::test]
async fn username_subscription_user_selection_keeps_typed_id_and_is_ineligible() {
    let (client, _runner, _dir) = super::super::stalled_client().await;
    let mut response = envelope(tl::enums::Chat::Channel(raw_tl_channel(7, "Channel", None)));
    response.peer = tl::enums::Peer::User(tl::types::PeerUser { user_id: 7 });
    response.users.push(tl::enums::User::User(raw_tl_user(
        7,
        Some("User"),
        None,
        None,
    )));
    let (resolved, _) = decode_resolved_peer(&client.client, response);
    let resolved = resolved.expect("typed user result");
    assert!(matches!(resolved.peer, Peer::User(_)));
    assert!(resolved.channel().is_none());
    let result = resolve_channel_with("channelname", async { panic!("no dialog walk") }, async {
        Ok(Some(resolved))
    })
    .await;
    assert!(matches!(result, Err(Error::UnsupportedPeer)));
}
