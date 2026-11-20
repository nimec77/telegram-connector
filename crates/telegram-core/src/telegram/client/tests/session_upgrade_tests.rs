//! Disposable offline fixtures for the grammers SQLite storage boundary.

use grammers_session::Session;
use grammers_session::storages::SqliteSession;
use grammers_session::types::{
    ChannelKind, ChannelState, PeerAuth, PeerId, PeerInfo, UpdatesState,
};

/// Schema copied from grammers-session 0.10 at 9fef0bae, without any real session data.
const LEGACY_V1_SCHEMA: &str = "
    CREATE TABLE dc_home (dc_id INTEGER NOT NULL, PRIMARY KEY(dc_id));
    CREATE TABLE dc_option (
        dc_id INTEGER NOT NULL, ipv4 TEXT NOT NULL, ipv6 TEXT NOT NULL,
        auth_key BLOB, PRIMARY KEY(dc_id));
    CREATE TABLE peer_info (
        peer_id INTEGER NOT NULL, hash INTEGER, subtype INTEGER, PRIMARY KEY(peer_id));
    CREATE TABLE update_state (
        pts INTEGER NOT NULL, qts INTEGER NOT NULL, date INTEGER NOT NULL, seq INTEGER NOT NULL);
    CREATE TABLE channel_state (
        peer_id INTEGER NOT NULL, pts INTEGER NOT NULL, PRIMARY KEY(peer_id));
    PRAGMA user_version = 1;
    INSERT INTO dc_home VALUES (4);
    INSERT INTO peer_info VALUES (7, 11, 2), (-7, NULL, NULL), (-1000000000007, 22, 8);
    INSERT INTO update_state VALUES (101, 102, 1700000000, 104);
    INSERT INTO channel_state VALUES (7, 105);
";

#[tokio::test]
async fn previous_v1_schema_reopens_with_peer_namespaces_and_update_state_intact() {
    let dir = tempfile::tempdir().expect("disposable legacy database directory");
    let path = dir.path().join("legacy.db");
    let database = libsql::Builder::new_local(&path)
        .build()
        .await
        .expect("offline SQLite builder");
    let connection = database.connect().expect("offline SQLite connection");
    connection
        .execute_batch(LEGACY_V1_SCHEMA)
        .await
        .expect("old schema fixture");
    drop(connection);
    drop(database);

    let session = SqliteSession::open(&path)
        .await
        .expect("old-format database opens");
    assert_eq!(session.home_dc_id().expect("home DC"), 4);
    for (id, expected) in [
        (
            PeerId::user_unchecked(7),
            PeerInfo::User {
                id: 7,
                auth: Some(PeerAuth::from_hash(11)),
                bot: Some(true),
                is_self: Some(false),
            },
        ),
        (PeerId::chat_unchecked(7), PeerInfo::Chat { id: 7 }),
        (
            PeerId::channel_unchecked(7),
            PeerInfo::Channel {
                id: 7,
                auth: Some(PeerAuth::from_hash(22)),
                kind: Some(ChannelKind::Broadcast),
            },
        ),
    ] {
        assert_eq!(session.peer(id).await.expect("cached peer"), Some(expected));
    }
    let expected = UpdatesState {
        pts: 101,
        qts: 102,
        date: 1_700_000_000,
        seq: 104,
        channels: vec![ChannelState { id: 7, pts: 105 }],
    };
    assert_eq!(
        session.updates_state().await.expect("update state"),
        expected
    );
    drop(session);
    let reopened = SqliteSession::open(&path).await.expect("second reopen");
    assert_eq!(
        reopened.updates_state().await.expect("persistent state"),
        expected
    );
}

#[tokio::test]
async fn peer_cache_refresh_prefers_new_information_and_preserves_missing_fields() {
    let dir = tempfile::tempdir().expect("disposable cache directory");
    let session = SqliteSession::open(dir.path().join("cache.db"))
        .await
        .expect("offline cache");
    let old = PeerInfo::Channel {
        id: 7,
        auth: Some(PeerAuth::from_hash(11)),
        kind: Some(ChannelKind::Megagroup),
    };
    session.cache_peer(old).await.expect("initial cache");
    let fresh = PeerInfo::Channel {
        id: 7,
        auth: Some(PeerAuth::from_hash(22)),
        kind: Some(ChannelKind::Broadcast),
    };
    session
        .cache_peer(fresh.clone())
        .await
        .expect("refreshed cache");
    assert_eq!(
        session
            .peer(PeerId::channel_unchecked(7))
            .await
            .expect("fresh peer"),
        Some(fresh.clone())
    );
    session
        .cache_peer(PeerInfo::Channel {
            id: 7,
            auth: None,
            kind: None,
        })
        .await
        .expect("partial update");
    assert_eq!(
        session
            .peer(PeerId::channel_unchecked(7))
            .await
            .expect("retained peer"),
        Some(fresh)
    );
}

#[tokio::test]
async fn bulk_cache_persists_all_peer_kinds_and_refreshed_values_after_reopen() {
    let dir = tempfile::tempdir().expect("disposable bulk-cache directory");
    let path = dir.path().join("bulk.db");
    let session = SqliteSession::open(&path)
        .await
        .expect("offline bulk cache");
    let peers = vec![
        PeerInfo::User {
            id: 7,
            auth: Some(PeerAuth::from_hash(11)),
            bot: Some(true),
            is_self: Some(false),
        },
        PeerInfo::Chat { id: 7 },
        PeerInfo::Channel {
            id: 7,
            auth: Some(PeerAuth::from_hash(22)),
            kind: Some(ChannelKind::Broadcast),
        },
        PeerInfo::Channel {
            id: 8,
            auth: Some(PeerAuth::from_hash(33)),
            kind: Some(ChannelKind::Megagroup),
        },
    ];
    session
        .cache_peers(peers.clone())
        .await
        .expect("all bulk peers stored");
    for peer in &peers {
        assert_eq!(
            session.peer(peer.id()).await.expect("bulk peer"),
            Some(peer.clone())
        );
    }
    let updated = PeerInfo::Channel {
        id: 7,
        auth: Some(PeerAuth::from_hash(99)),
        kind: Some(ChannelKind::Megagroup),
    };
    session
        .cache_peers(vec![updated.clone()])
        .await
        .expect("refresh batch");
    session
        .cache_peers(vec![PeerInfo::Channel {
            id: 7,
            auth: None,
            kind: None,
        }])
        .await
        .expect("partial batch");
    drop(session);
    let reopened = SqliteSession::open(&path)
        .await
        .expect("persistent bulk cache");
    for peer in peers {
        let expected = if peer.id() == updated.id() {
            updated.clone()
        } else {
            peer
        };
        assert_eq!(
            reopened
                .peer(expected.id())
                .await
                .expect("reopened bulk peer"),
            Some(expected)
        );
    }
}

#[tokio::test]
async fn reviewed_revision_sqlite_community_roundtrip_preserves_auth_but_loses_kind() {
    // Known upstream limitation in both reviewed pins: subtype 16 is written,
    // but the SQLite reader has no Community branch. This is an observation
    // for the pinned migration, not a claim that the metadata loss is desirable.
    let dir = tempfile::tempdir().expect("disposable community directory");
    let path = dir.path().join("community.db");
    let session = SqliteSession::open(&path)
        .await
        .expect("offline community cache");
    session
        .cache_peer(PeerInfo::Channel {
            id: 9,
            auth: Some(PeerAuth::from_hash(44)),
            kind: Some(ChannelKind::Community),
        })
        .await
        .expect("community cached");
    drop(session);
    let reopened = SqliteSession::open(&path)
        .await
        .expect("reopened community cache");
    assert_eq!(
        reopened
            .peer(PeerId::channel_unchecked(9))
            .await
            .expect("community identity"),
        Some(PeerInfo::Channel {
            id: 9,
            auth: Some(PeerAuth::from_hash(44)),
            kind: None
        })
    );
}

#[tokio::test]
async fn reviewed_revision_sqlite_user_false_flags_reopen_as_unknown() {
    // The existing SQLite subtype encoding stores false/false as NULL. Keep
    // this limitation explicit while independently checking identity/auth.
    let dir = tempfile::tempdir().expect("disposable user directory");
    let path = dir.path().join("user.db");
    let session = SqliteSession::open(&path)
        .await
        .expect("offline user cache");
    session
        .cache_peer(PeerInfo::User {
            id: 9,
            auth: Some(PeerAuth::from_hash(44)),
            bot: Some(false),
            is_self: Some(false),
        })
        .await
        .expect("user cached");
    drop(session);
    let reopened = SqliteSession::open(&path)
        .await
        .expect("reopened user cache");
    assert_eq!(
        reopened
            .peer(PeerId::user_unchecked(9))
            .await
            .expect("user identity"),
        Some(PeerInfo::User {
            id: 9,
            auth: Some(PeerAuth::from_hash(44)),
            bot: None,
            is_self: None
        })
    );
}
