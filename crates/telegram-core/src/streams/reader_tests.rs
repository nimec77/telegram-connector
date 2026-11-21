//! Native checkpoint recovery and difference-boundary fixtures.

use super::{DifferenceTarget, MAX_CONSECUTIVE_DIFFERENCES, count_difference};

#[test]
fn final_channel_difference_replays_two_edits_before_deletion_after_restart() {
    use super::apply_channel_batch;
    use crate::test_helpers::raw_tl_message_grouped;
    use grammers_client::tl;
    use grammers_session::types::{ChannelState, UpdatesState};
    use grammers_session::updates::{MessageBox, MessageBoxes};

    let restored = UpdatesState {
        channels: vec![ChannelState { id: 42, pts: 100 }],
        ..UpdatesState::default()
    };
    let mut boxes = MessageBoxes::load(restored);
    let request = boxes
        .get_channel_difference()
        .expect("restore difference request");
    assert_eq!(request.pts, 100);
    let raw = |text: &str, edit_date| {
        let tl::enums::Message::Message(mut message) =
            raw_tl_message_grouped(104, 1_795_078_800, 42, 1)
        else {
            panic!("plain fixture");
        };
        message.grouped_id = None;
        message.post = false;
        message.message = text.into();
        message.edit_date = edit_date;
        tl::enums::Message::Message(message)
    };
    let edit = |text, pts| {
        tl::types::UpdateEditChannelMessage {
            message: raw(text, Some(1_795_078_801)),
            pts,
            pts_count: 1,
        }
        .into()
    };
    let response = tl::types::updates::ChannelDifference {
        r#final: true,
        pts: 104,
        timeout: None,
        new_messages: vec![raw("original", None)],
        other_updates: vec![
            tl::types::UpdateDeleteChannelMessages {
                channel_id: 42,
                messages: vec![104],
                pts: 104,
                pts_count: 1,
            }
            .into(),
            edit("second", 103),
            edit("first", 102),
            edit("old", 100),
        ],
        chats: Vec::new(),
        users: Vec::new(),
    };
    let (updates, _, _) =
        apply_channel_batch(&mut boxes, response.into(), 42, request.pts, Some(&[42]));
    let changes: Vec<_> = updates
        .into_iter()
        .map(|(update, state)| match update {
            tl::enums::Update::NewChannelMessage(added) => ("added".into(), added.pts),
            tl::enums::Update::EditChannelMessage(edit) => {
                let tl::enums::Message::Message(raw) = edit.message else {
                    panic!("edit body")
                };
                assert_eq!(raw.edit_date, Some(1_795_078_801));
                assert_eq!(
                    state.message_box,
                    Some(MessageBox::Channel {
                        channel_id: 42,
                        pts: edit.pts
                    })
                );
                (raw.message, edit.pts)
            }
            tl::enums::Update::DeleteChannelMessages(deleted) => ("deleted".into(), deleted.pts),
            _ => panic!("unexpected recovered update"),
        })
        .collect();
    assert_eq!(
        changes,
        [
            ("added".into(), 0),
            ("first".into(), 102),
            ("second".into(), 103),
            ("deleted".into(), 104)
        ]
    );
    assert_eq!(
        boxes.session_state().channels,
        [ChannelState { id: 42, pts: 104 }]
    );
    assert!(boxes.get_channel_difference().is_none());
    assert_eq!(
        MessageBoxes::load(boxes.session_state())
            .get_channel_difference()
            .expect("next restart position")
            .pts,
        104
    );
}

#[test]
fn recovery_orders_a_basic_addition_before_a_common_deletion_with_colliding_ids() {
    use super::recovered::order_recovered_additions;
    use crate::test_helpers::raw_tl_message_grouped;
    use grammers_client::tl;
    use grammers_session::updates::State;

    let tl::enums::Message::Message(mut group) = raw_tl_message_grouped(104, 0, 42, 1) else {
        panic!("plain group fixture");
    };
    group.post = false;
    let supergroup = group.clone();
    group.peer_id = tl::types::PeerChat { chat_id: 42 }.into();
    let state = State {
        date: 0,
        seq: 0,
        message_box: None,
    };
    let mut batch = vec![
        (
            tl::types::UpdateDeleteMessages {
                messages: vec![104],
                pts: 102,
                pts_count: 1,
            }
            .into(),
            state.clone(),
        ),
        (
            tl::types::UpdateNewChannelMessage {
                message: supergroup.into(),
                pts: 0,
                pts_count: 0,
            }
            .into(),
            state.clone(),
        ),
        (
            tl::types::UpdateNewMessage {
                message: group.into(),
                pts: 0,
                pts_count: 0,
            }
            .into(),
            state,
        ),
    ];
    order_recovered_additions(&mut batch);
    assert!(matches!(&batch[0].0, tl::enums::Update::NewMessage(_)));
    assert!(matches!(&batch[1].0, tl::enums::Update::DeleteMessages(_)));
    assert!(matches!(
        &batch[2].0,
        tl::enums::Update::NewChannelMessage(_)
    ));
}

#[test]
fn secondary_difference_replays_basic_group_edits_and_common_deletion() {
    use super::apply_common_batch;
    use crate::test_helpers::raw_tl_message_grouped;
    use grammers_client::tl;
    use grammers_session::types::UpdatesState;
    use grammers_session::updates::{MessageBox, MessageBoxes};

    // Only Secondary enters the pinned SDK's force-apply set.
    let mut boxes = MessageBoxes::load(UpdatesState {
        qts: 11,
        ..UpdatesState::default()
    });
    let tl::enums::Message::Message(mut group) = raw_tl_message_grouped(7, 1_795_078_800, 42, 1)
    else {
        panic!("plain group fixture");
    };
    group.post = false;
    group.grouped_id = None;
    group.peer_id = tl::types::PeerChat { chat_id: 42 }.into();
    group.message = "addition".into();
    let mut edited = group.clone();
    edited.edit_date = Some(1_795_078_801);
    edited.message = "edit".into();
    let actual = tl::types::updates::Difference {
        new_messages: vec![group.into()],
        new_encrypted_messages: vec![],
        other_updates: vec![
            tl::types::UpdateDeleteMessages {
                messages: vec![7],
                pts: 103,
                pts_count: 1,
            }
            .into(),
            tl::types::UpdateEditMessage {
                message: edited.into(),
                pts: 102,
                pts_count: 1,
            }
            .into(),
        ],
        chats: vec![],
        users: vec![],
        state: tl::types::updates::State {
            pts: 103,
            qts: 12,
            date: 1_795_078_802,
            seq: 5,
            unread_count: 0,
        }
        .into(),
    };
    let (updates, _, _) = apply_common_batch(&mut boxes, actual.into(), 100, Some(&[]));
    assert!(matches!(&updates[0].0, tl::enums::Update::NewMessage(_)));
    assert!(matches!(&updates[1].0, tl::enums::Update::EditMessage(edit) if edit.pts == 102));
    assert_eq!(
        updates[1].1.message_box,
        Some(MessageBox::Common { pts: 102 })
    );
    assert!(
        matches!(&updates[2].0, tl::enums::Update::DeleteMessages(delete) if delete.pts == 103)
    );
    assert_eq!(updates.len(), 3);
    assert_eq!(boxes.session_state().pts, 103);
}

#[tokio::test(start_paused = true)]
async fn group_bootstrap_preserves_known_positions_when_other_peers_stall() {
    use super::{GROUP_BOOTSTRAP_BUDGET, seed_channels};
    use crate::telegram::client::stalled_client;
    use grammers_session::types::{ChannelState, UpdatesState};
    use grammers_session::updates::MessageBoxes;
    use std::time::Duration;
    use tokio::time::Instant;

    let (client, _runner, _root) = stalled_client().await;
    let known = ChannelState { id: 42, pts: 100 };
    let mut boxes = MessageBoxes::load(UpdatesState {
        channels: vec![known.clone()],
        ..UpdatesState::default()
    });
    let groups = (42..=60).collect::<Vec<_>>();
    let start = Instant::now();
    let retry = seed_channels(&client, &mut boxes, &groups, Duration::from_secs(30)).await;
    assert_eq!(retry.first(), Some(&47));
    let mut retry = retry;
    retry.sort_unstable();
    assert_eq!(retry, groups[1..]);
    assert_eq!(start.elapsed(), GROUP_BOOTSTRAP_BUDGET);
    assert_eq!(boxes.session_state().channels, [known]);
}

#[test]
fn too_long_notice_recovers_from_the_last_observed_channel_position() {
    use super::retain_channel_checkpoints;
    use grammers_client::tl;
    use grammers_session::updates::{MessageBoxes, UpdatesLike};

    let mut boxes = MessageBoxes::new();
    boxes.try_set_channel_state(42, 100);
    let too_long = tl::types::UpdateChannelTooLong {
        channel_id: 42,
        pts: Some(110),
    };
    let mut updates = [tl::enums::Update::from(too_long)];
    retain_channel_checkpoints(&mut updates, &boxes.session_state(), &[42]);
    boxes
        .process_updates(UpdatesLike::Updates(tl::enums::Updates::UpdateShort(
            tl::types::UpdateShort {
                update: updates.into_iter().next().expect("one update"),
                date: 0,
            },
        )))
        .expect("sequence notification");
    assert_eq!(
        boxes
            .get_channel_difference()
            .expect("request missing updates")
            .pts,
        100
    );
}

#[tokio::test]
async fn failed_journal_append_leaves_checkpoint_available_for_recovery() {
    use super::{ReadFailure, checkpoint_observed_batch};
    use crate::error::Error;
    use crate::streams::{UpdateObserver, UpdatePosition};
    use grammers_client::tl;
    use grammers_session::Session;
    use grammers_session::storages::MemorySession;
    use grammers_session::types::UpdatesState;
    use std::time::Duration;

    struct Journal(bool);

    #[async_trait::async_trait]
    impl UpdateObserver for Journal {
        async fn started(&self) -> Result<(), Error> {
            Ok(())
        }

        async fn observe(
            &self,
            _updates: &[(tl::enums::Update, UpdatePosition)],
            _users: &[tl::enums::User],
            _chats: &[tl::enums::Chat],
            _recovering: bool,
        ) -> Result<(), Error> {
            if self.0 {
                Ok(())
            } else {
                Err(Error::TelegramApi("fixture journal write failed".into()))
            }
        }

        async fn disconnected(&self, _: Option<i64>, _: bool) -> Result<(), Error> {
            Ok(())
        }
    }

    let session = MemorySession::default();
    let next = UpdatesState {
        pts: 42,
        ..UpdatesState::default()
    };
    let batch = (Vec::new(), Vec::new(), Vec::new());
    let verify_checkpoint = async |observer: Journal| {
        checkpoint_observed_batch(
            &observer,
            &session,
            next.clone(),
            &batch,
            false,
            Duration::from_secs(1),
        )
        .await
    };
    assert!(matches!(
        verify_checkpoint(Journal(false)).await,
        Err(ReadFailure::Journal)
    ));
    assert_eq!(session.updates_state().await.expect("initial state").pts, 0);

    verify_checkpoint(Journal(true))
        .await
        .expect("commit after successful append");
    assert_eq!(
        session.updates_state().await.expect("committed state").pts,
        42
    );
}

#[test]
fn unrelated_channels_have_independent_difference_page_allowances() {
    let mut differences = None;
    for channel in 1..=20 {
        assert!(count_difference(&mut differences, DifferenceTarget::Channel(channel)).is_ok());
    }
    for _ in 1..MAX_CONSECUTIVE_DIFFERENCES {
        assert!(count_difference(&mut differences, DifferenceTarget::Channel(20)).is_ok());
    }
    assert!(count_difference(&mut differences, DifferenceTarget::Channel(20)).is_err());
    assert!(count_difference(&mut differences, DifferenceTarget::Common).is_ok());
}
