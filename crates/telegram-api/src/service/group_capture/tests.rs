use super::*;
use crate::service::group_journal::{PageResult, Retention};
use telegram_core::test_helpers::{raw_tl_message, raw_tl_message_grouped, raw_tl_user};

const CHAT: GroupRef = GroupRef::Chat(42);
const SUPERGROUP: GroupRef = GroupRef::Supergroup(42);
const GENERATION: &str = "test-generation";

fn peer(group: GroupRef) -> tl::enums::Peer {
    match group {
        GroupRef::Chat(chat_id) => tl::types::PeerChat { chat_id }.into(),
        GroupRef::Supergroup(channel_id) => tl::types::PeerChannel { channel_id }.into(),
    }
}

fn message(group: GroupRef, id: i32, text: &str) -> tl::types::Message {
    let tl::enums::Message::Message(mut raw) = raw_tl_message_grouped(id, 1_700_000_000, 42, 123)
    else {
        panic!("fixture expects an ordinary message");
    };
    raw.peer_id = peer(group);
    raw.from_id = Some(tl::types::PeerUser { user_id: 5 }.into());
    raw.message = text.into();
    raw.grouped_id = None;
    raw.post = false;
    raw
}

fn reply(parent_peer: Option<tl::enums::Peer>) -> tl::enums::MessageReplyHeader {
    tl::types::MessageReplyHeader {
        reply_to_scheduled: false,
        forum_topic: false,
        quote: false,
        reply_to_ephemeral: false,
        reply_to_msg_id: Some(75),
        reply_to_peer_id: parent_peer,
        reply_from: None,
        reply_media: None,
        reply_to_top_id: None,
        quote_text: None,
        quote_entities: None,
        quote_offset: None,
        todo_item_id: None,
        poll_option: None,
        quote_rich: None,
    }
    .into()
}

fn update(message: tl::types::Message, group: GroupRef, pts: i32, edit: bool) -> tl::enums::Update {
    match (group, edit) {
        (GroupRef::Chat(_), false) => tl::types::UpdateNewMessage {
            message: message.into(),
            pts,
            pts_count: 1,
        }
        .into(),
        (GroupRef::Chat(_), true) => tl::types::UpdateEditMessage {
            message: message.into(),
            pts,
            pts_count: 1,
        }
        .into(),
        (GroupRef::Supergroup(_), false) => tl::types::UpdateNewChannelMessage {
            message: message.into(),
            pts,
            pts_count: 1,
        }
        .into(),
        (GroupRef::Supergroup(_), true) => tl::types::UpdateEditChannelMessage {
            message: message.into(),
            pts,
            pts_count: 1,
        }
        .into(),
    }
}

fn frame(update: tl::enums::Update) -> (tl::enums::Update, UpdatePosition) {
    (
        update,
        UpdatePosition {
            date: 0,
            seq: 0,
            message_box: None,
        },
    )
}

async fn fixture() -> (tempfile::TempDir, Arc<GroupJournal>, Capture) {
    let root = tempfile::tempdir().expect("private journal fixture");
    let journal = Arc::new(
        GroupJournal::open(
            root.path(),
            Retention {
                seconds: 86_400,
                bytes: 1024 * 1024,
            },
        )
        .await
        .expect("open journal"),
    );
    let capture = Capture::new(
        Arc::clone(&journal),
        GENERATION.to_owned(),
        [CHAT, SUPERGROUP].into(),
        [CHAT, SUPERGROUP].into(),
        Arc::new(Notify::new()),
        Arc::new(AtomicBool::new(false)),
    );
    capture.started().await.expect("start group capture");
    (root, journal, capture)
}

async fn page(journal: &GroupJournal, group: GroupRef) -> crate::contracts::groups::Page {
    match journal
        .page(GENERATION, "test-owner", &group.to_string(), None, 100)
        .await
        .expect("read capture journal")
    {
        PageResult::Page(page) => page,
        _ => panic!("fixture expected a page"),
    }
}

#[tokio::test]
async fn captures_both_peer_namespaces_with_distinct_same_second_edits() {
    let (_root, journal, capture) = fixture().await;
    let author = raw_tl_user(5, Some("Test"), Some("Driver"), Some("driver")).into();

    for group in [CHAT, SUPERGROUP] {
        let mut first = message(group, 104, "first");
        first.reply_to = Some(reply(None));
        let mut external = message(group, 105, "external");
        let other_group = if group == CHAT { SUPERGROUP } else { CHAT };
        external.reply_to = Some(reply(Some(peer(other_group))));
        let mut edit = first.clone();
        edit.edit_date = Some(1_700_000_001);
        edit.message = "second".into();
        let mut last = edit.clone();
        last.message = "last".into();
        let deletion = match group {
            GroupRef::Chat(_) => tl::types::UpdateDeleteMessages {
                messages: vec![104],
                pts: 5,
                pts_count: 1,
            }
            .into(),
            GroupRef::Supergroup(channel_id) => tl::types::UpdateDeleteChannelMessages {
                channel_id,
                messages: vec![104],
                pts: 5,
                pts_count: 1,
            }
            .into(),
        };
        let updates = [
            frame(update(first, group, 1, false)),
            frame(update(external, group, 2, false)),
            frame(update(edit, group, 3, true)),
            frame(update(last, group, 4, true)),
            frame(deletion),
        ];
        capture
            .observe(&updates, std::slice::from_ref(&author), &[], false)
            .await
            .expect("record updates");
        capture
            .observe(&updates, std::slice::from_ref(&author), &[], false)
            .await
            .expect("replayed updates deduplicate");

        let entries = page(&journal, group).await.entries;
        assert_eq!(entries.len(), 5);
        assert!(matches!(&entries[0].change, Change::Added {
            message_id, reply_kind: ReplyKind::Local, reply_to_message_id: Some(parent),
            author: Author {kind: AuthorKind::User, user_id: Some(author_id), display_name: Some(name), ..}, ..
        } if message_id == "104" && parent == "75" && author_id == "5" && name == "Test Driver"));
        assert!(matches!(
            &entries[1].change,
            Change::Added {
                reply_kind: ReplyKind::Unavailable,
                reply_to_message_id: None,
                ..
            }
        ));
        assert!(
            matches!(&entries[2].change, Change::Edited { text, edited_at, .. } if text == "second" && edited_at == "2023-11-14T22:13:21Z")
        );
        assert!(
            matches!(&entries[3].change, Change::Edited { text, edited_at, .. } if text == "last" && edited_at == "2023-11-14T22:13:21Z")
        );
        assert!(
            matches!(&entries[4].change, Change::Deleted { message_id, deleted_at } if message_id == "104" && deleted_at == &entries[4].observed_at)
        );
    }
}

#[tokio::test]
async fn filters_private_codes_and_captioned_media_before_journaling() {
    let (_root, journal, capture) = fixture().await;
    let mut private = message(CHAT, 1, "synthetic private code 4123");
    private.peer_id = tl::types::PeerUser { user_id: 99 }.into();
    let mut photo = message(CHAT, 2, "synthetic caption");
    photo.media = Some(
        tl::types::MessageMediaPhoto {
            spoiler: false,
            live_photo: false,
            photo: None,
            ttl_seconds: None,
            video: None,
        }
        .into(),
    );
    let mut voice = message(SUPERGROUP, 3, "synthetic voice caption");
    voice.media = Some(
        tl::types::MessageMediaDocument {
            nopremium: false,
            spoiler: false,
            video: false,
            round: false,
            voice: true,
            document: None,
            alt_documents: None,
            video_cover: None,
            video_timestamp: None,
            ttl_seconds: None,
        }
        .into(),
    );
    let service = tl::types::UpdateNewChannelMessage {
        message: raw_tl_message(4, 1_700_000_000, 42),
        pts: 2,
        pts_count: 1,
    };
    let deletion = tl::types::UpdateDeleteMessages {
        messages: vec![1, 2],
        pts: 4,
        pts_count: 2,
    };
    let updates = [
        frame(update(private, CHAT, 1, false)),
        frame(update(photo, CHAT, 2, false)),
        frame(update(voice, SUPERGROUP, 1, false)),
        frame(service.into()),
        frame(deletion.into()),
        frame(
            tl::types::UpdateDeleteChannelMessages {
                channel_id: 42,
                messages: vec![3, 4],
                pts: 4,
                pts_count: 2,
            }
            .into(),
        ),
    ];
    capture
        .observe(&updates, &[], &[], false)
        .await
        .expect("handle excluded updates");
    assert!(page(&journal, CHAT).await.entries.is_empty());
    assert!(page(&journal, SUPERGROUP).await.entries.is_empty());
}

#[tokio::test]
async fn unattributed_common_deletion_has_no_guessed_target() {
    let (_root, journal, capture) = fixture().await;
    let deletion = tl::types::UpdateDeleteMessages {
        messages: vec![692],
        pts: 1,
        pts_count: 1,
    };
    capture
        .observe(&[frame(deletion.into())], &[], &[], false)
        .await
        .expect("persist coverage gap");
    let entries = page(&journal, CHAT).await.entries;
    assert_eq!(entries.len(), 1);
    assert!(matches!(
        entries[0].change,
        Change::Gap {
            reason: GapReason::AmbiguousUpdate
        }
    ));
    assert!(page(&journal, SUPERGROUP).await.entries.is_empty());
}

#[tokio::test]
async fn new_stream_skips_startup_history_and_replacement_preserves_failure_marker() {
    let (_root, journal, capture) = fixture().await;
    let older = [frame(update(
        message(CHAT, 10, "historical"),
        CHAT,
        1,
        false,
    ))];
    capture
        .observe(&older, &[], &[], true)
        .await
        .expect("ignore newly enabled group history during startup");
    assert_eq!(page(&journal, CHAT).await.base_seq, "0");
    assert!(page(&journal, CHAT).await.entries.is_empty());

    let live = [frame(update(message(CHAT, 11, "live"), CHAT, 2, false))];
    capture
        .observe(&live, &[], &[], false)
        .await
        .expect("journal a live update");

    // The service preserves this indication across reader lifetimes when its
    // journal could not accept the failure marker.
    let pending = Arc::new(AtomicBool::new(true));
    let replacement = Capture::new(
        Arc::clone(&journal),
        GENERATION.to_owned(),
        [CHAT].into(),
        [CHAT].into(),
        Arc::new(Notify::new()),
        Arc::clone(&pending),
    );
    replacement.started().await.expect("replace capture reader");
    replacement
        .observe(&live, &[], &[], true)
        .await
        .expect("process duplicate replay");
    assert!(!pending.load(Ordering::Acquire));

    let entries = page(&journal, CHAT).await.entries;
    assert_eq!(entries.len(), 2);
    assert!(matches!(&entries[0].change, Change::Added { text, .. } if text == "live"));
    assert!(matches!(
        entries[1].change,
        Change::Gap {
            reason: GapReason::JournalFailure
        }
    ));
}

#[tokio::test]
async fn live_membership_loss_blocks_capture_until_join_is_confirmed() {
    let (_root, journal, capture) = fixture().await;
    assert!(capture.is_monitoring());
    let mut raw = telegram_core::test_helpers::raw_tl_channel(42, "Fixture", None);
    raw.broadcast = false;
    raw.megagroup = true;
    raw.left = true;
    let updates = [frame(update(
        message(SUPERGROUP, 1, "while unjoined"),
        SUPERGROUP,
        1,
        false,
    ))];
    let basic_left = tl::types::ChatForbidden {
        id: 42,
        title: "Basic fixture".into(),
    };
    capture
        .observe_socket_membership(&[raw.clone().into(), basic_left.into()])
        .await
        .expect("record lost coverage even before update sequencing");
    assert!(!capture.is_monitoring());
    capture
        .observe(&updates, &[], &[], false)
        .await
        .expect("skip unjoined group");
    capture
        .membership_lost(42)
        .await
        .expect("repeat negative evidence is idempotent");

    raw.left = false;
    let updates = [frame(update(
        message(SUPERGROUP, 2, "after rejoin"),
        SUPERGROUP,
        2,
        false,
    ))];
    capture
        .observe(&updates, &[], &[raw.into()], false)
        .await
        .expect("capture after confirmed rejoin");
    assert!(capture.is_monitoring());

    let entries = page(&journal, SUPERGROUP).await.entries;
    assert_eq!(entries.len(), 2);
    assert!(matches!(
        entries[0].change,
        Change::Gap {
            reason: GapReason::UpstreamDisconnect
        }
    ));
    assert!(matches!(&entries[1].change, Change::Added { text, .. } if text == "after rejoin"));
}

#[tokio::test]
async fn membership_refresh_is_bounded_and_discards_stale_results() {
    let (_root, journal, capture) = fixture().await;
    tokio::time::pause();
    capture
        .refresh_memberships(async { panic!("capture already checked membership at startup") })
        .await
        .expect("recent lookup bypasses polling");

    tokio::time::advance(Duration::from_secs(60)).await;
    capture
        .refresh_memberships(async {
            capture
                .membership_lost(42)
                .await
                .expect("newer authoritative membership loss");
            [CHAT, SUPERGROUP].into()
        })
        .await
        .expect("ignore stale membership result");
    let while_unjoined = [frame(update(
        message(SUPERGROUP, 7, "was not a member"),
        SUPERGROUP,
        1,
        false,
    ))];
    capture
        .observe(&while_unjoined, &[], &[], false)
        .await
        .expect("keep refusing capture after stale probe");
    assert_eq!(page(&journal, SUPERGROUP).await.entries.len(), 1);

    capture
        .refresh_memberships(async { panic!("a stale result still used a lookup attempt") })
        .await
        .expect("avoid immediate probing after stale result");

    tokio::time::advance(Duration::from_secs(60)).await;
    capture
        .refresh_memberships(async { [SUPERGROUP].into() })
        .await
        .expect("refresh joined set");
    let live = [frame(update(
        message(SUPERGROUP, 8, "joined again"),
        SUPERGROUP,
        2,
        false,
    ))];
    capture
        .observe(&live, &[], &[], false)
        .await
        .expect("capture after successful probe");
    assert_eq!(page(&journal, SUPERGROUP).await.entries.len(), 2);
    assert!(
        matches!(page(&journal, CHAT).await.entries.as_slice(), [entry]
        if matches!(entry.change, Change::Gap { reason: GapReason::UpstreamDisconnect }))
    );
}

#[tokio::test]
async fn membership_loss_stops_capture_even_when_its_gap_cannot_be_written() {
    let (_root, journal, capture) = fixture().await;
    journal
        .retire(GENERATION)
        .await
        .expect("simulate a journal refusing writes for this generation");

    let basic_left = tl::types::ChatForbidden {
        id: 42,
        title: "Basic fixture".into(),
    };
    let channel_left = tl::types::ChannelForbidden {
        broadcast: false,
        megagroup: true,
        monoforum: false,
        id: 42,
        access_hash: 1,
        title: "Supergroup fixture".into(),
        until_date: None,
    };
    assert!(
        capture
            .observe_socket_membership(&[basic_left.into(), channel_left.into()])
            .await
            .is_err()
    );
    assert!(!capture.is_monitoring());
    assert!(capture.pending_journal_gap.load(Ordering::Acquire));
}

#[tokio::test]
async fn synthetic_edited_snapshot_does_not_invent_original_text() {
    let (_root, journal, capture) = fixture().await;
    let observed_add = frame(update(message(CHAT, 21, "original"), CHAT, 1, false));
    capture
        .observe(&[observed_add], &[], &[], false)
        .await
        .expect("journal independently observed original");

    let mut edited_snapshot = message(CHAT, 21, "snapshot of an observed original");
    edited_snapshot.edit_date = Some(1_700_000_001);
    let mut unseen_snapshot = message(CHAT, 22, "snapshot of a missing original");
    unseen_snapshot.edit_date = edited_snapshot.edit_date;
    let observed_edit = unseen_snapshot.clone();
    let recovered = [
        frame(update(edited_snapshot, CHAT, 0, false)),
        frame(update(unseen_snapshot, CHAT, 0, false)),
        frame(update(observed_edit, CHAT, 2, true)),
        frame(
            tl::types::UpdateDeleteMessages {
                messages: vec![22],
                pts: 3,
                pts_count: 1,
            }
            .into(),
        ),
    ];
    for _ in 0..2 {
        capture
            .observe(&recovered, &[], &[], false)
            .await
            .expect("journal replay without copying the edited snapshot into an add");
    }
    let entries = page(&journal, CHAT).await.entries;
    assert_eq!(entries.len(), 4);
    assert!(matches!(&entries[0].change, Change::Added { text, .. } if text == "original"));
    assert!(matches!(
        &entries[1].change,
        Change::Gap {
            reason: GapReason::UpstreamDisconnect
        }
    ));
    assert!(
        matches!(&entries[2].change, Change::Edited { text, .. } if text == "snapshot of a missing original")
    );
    assert!(matches!(&entries[3].change, Change::Deleted { message_id, .. } if message_id == "22"));
}
