//! Recover immutable message changes alongside the SDK's difference bookkeeping.

use super::retain_channel_checkpoints;
use grammers_client::tl;
use grammers_session::updates::{MessageBox, MessageBoxes, State, UpdateAndPeers};
use std::collections::HashMap;

pub(super) fn apply_common_batch(
    boxes: &mut MessageBoxes,
    mut response: tl::enums::updates::Difference,
    previous_pts: i32,
    observed_channels: Option<&[i64]>,
) -> UpdateAndPeers {
    let mut recovered = Vec::new();
    if let Some(channels) = observed_channels {
        let page = match &mut response {
            tl::enums::updates::Difference::Difference(page) => {
                Some((&mut page.other_updates, &page.state))
            }
            tl::enums::updates::Difference::Slice(page) => {
                Some((&mut page.other_updates, &page.intermediate_state))
            }
            _ => None,
        };
        if let Some((updates, tl::enums::updates::State::State(state))) = page {
            retain_channel_checkpoints(updates, &boxes.session_state(), channels);
            // A difference for the Secondary box still updates Common's pts.
            // Recover group changes before the SDK's normal pts filter runs.
            recovered = take_ordered_changes(updates, previous_pts, state.pts, common_change);
        }
    }
    let mut batch = boxes.apply_difference(response);
    if observed_channels.is_some() {
        append_recovered(&mut batch, boxes, recovered, |pts| MessageBox::Common {
            pts,
        });
        order_recovered_additions(&mut batch.0);
    }
    batch
}

pub(super) fn apply_channel_batch(
    boxes: &mut MessageBoxes,
    mut response: tl::enums::updates::ChannelDifference,
    channel_id: i64,
    previous_pts: i32,
    observed_channels: Option<&[i64]>,
) -> UpdateAndPeers {
    let mut recovered = Vec::new();
    if let tl::enums::updates::ChannelDifference::Difference(difference) = &mut response {
        difference.new_messages.sort_by_key(tl::enums::Message::id);
        if let Some(channels) = observed_channels {
            retain_channel_checkpoints(
                &mut difference.other_updates,
                &boxes.session_state(),
                channels,
            );
            if channels.contains(&channel_id) {
                // The SDK advances to the final pts before filtering other_updates
                // on a final page. Keep the matching channel's source positions.
                recovered = take_ordered_changes(
                    &mut difference.other_updates,
                    previous_pts,
                    difference.pts,
                    |update| channel_change(update, channel_id),
                );
            }
        }
    }
    let mut batch = boxes.apply_channel_difference(response);
    if observed_channels.is_some() {
        append_recovered(&mut batch, boxes, recovered, |pts| MessageBox::Channel {
            channel_id,
            pts,
        });
        order_recovered_additions(&mut batch.0);
    }
    batch
}

fn take_ordered_changes(
    updates: &mut Vec<tl::enums::Update>,
    previous_pts: i32,
    final_pts: i32,
    position: impl Fn(&tl::enums::Update) -> Option<(i32, i32)>,
) -> Vec<(i32, tl::enums::Update)> {
    let mut recovered: Vec<_> = updates
        .extract_if(.., |update| position(update).is_some())
        .filter_map(|update| {
            let (pts, count) = position(&update)?;
            (count > 0 && pts <= final_pts && pts.checked_sub(count)? >= previous_pts)
                .then_some((pts, update))
        })
        .collect();
    recovered.sort_by_key(|(pts, _)| *pts);
    recovered
}

fn append_recovered(
    batch: &mut UpdateAndPeers,
    boxes: &MessageBoxes,
    recovered: Vec<(i32, tl::enums::Update)>,
    message_box: impl Fn(i32) -> MessageBox,
) {
    if recovered.is_empty() {
        return;
    }
    let position = boxes.session_state();
    batch.0.extend(recovered.into_iter().map(|(pts, update)| {
        (
            update,
            State {
                date: position.date,
                seq: position.seq,
                message_box: Some(message_box(pts)),
            },
        )
    }));
}

fn common_change(update: &tl::enums::Update) -> Option<(i32, i32)> {
    match update {
        tl::enums::Update::EditMessage(edit)
            if matches!(&edit.message, tl::enums::Message::Message(raw)
                if matches!(&raw.peer_id, tl::enums::Peer::Chat(_))) =>
        {
            Some((edit.pts, edit.pts_count))
        }
        tl::enums::Update::DeleteMessages(delete) => Some((delete.pts, delete.pts_count)),
        _ => None,
    }
}

fn channel_change(update: &tl::enums::Update, channel_id: i64) -> Option<(i32, i32)> {
    match update {
        tl::enums::Update::EditChannelMessage(edit)
            if matches!(&edit.message, tl::enums::Message::Message(raw)
                if matches!(&raw.peer_id, tl::enums::Peer::Channel(peer) if peer.channel_id == channel_id)) =>
        {
            Some((edit.pts, edit.pts_count))
        }
        tl::enums::Update::DeleteChannelMessages(delete) if delete.channel_id == channel_id => {
            Some((delete.pts, delete.pts_count))
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum MessageKey {
    Basic(i64, i32),
    Supergroup(i64, i32),
    CommonDeletion(i32),
}

fn key(message: &tl::enums::Message) -> Option<MessageKey> {
    let tl::enums::Message::Message(raw) = message else {
        return None;
    };
    match &raw.peer_id {
        tl::enums::Peer::Chat(peer) => Some(MessageKey::Basic(peer.chat_id, raw.id)),
        tl::enums::Peer::Channel(peer) if !raw.post => {
            Some(MessageKey::Supergroup(peer.channel_id, raw.id))
        }
        _ => None,
    }
}

pub(super) fn order_recovered_additions(updates: &mut Vec<(tl::enums::Update, State)>) {
    let mut first_changes = HashMap::new();
    for (index, (update, _)) in updates.iter().enumerate() {
        match update {
            tl::enums::Update::EditMessage(edit) => {
                if let Some(key) = key(&edit.message) {
                    first_changes.entry(key).or_insert(index);
                }
            }
            tl::enums::Update::EditChannelMessage(edit) => {
                if let Some(key) = key(&edit.message) {
                    first_changes.entry(key).or_insert(index);
                }
            }
            tl::enums::Update::DeleteMessages(delete) => {
                for &id in &delete.messages {
                    first_changes
                        .entry(MessageKey::CommonDeletion(id))
                        .or_insert(index);
                }
            }
            tl::enums::Update::DeleteChannelMessages(delete) => {
                for &id in &delete.messages {
                    first_changes
                        .entry(MessageKey::Supergroup(delete.channel_id, id))
                        .or_insert(index);
                }
            }
            _ => (),
        }
    }
    if first_changes.is_empty() {
        return;
    }
    let mut ordered = Vec::with_capacity(updates.len());
    for (index, entry) in std::mem::take(updates).into_iter().enumerate() {
        let addition = match &entry.0 {
            tl::enums::Update::NewMessage(add) if add.pts == 0 => key(&add.message),
            tl::enums::Update::NewChannelMessage(add) if add.pts == 0 => key(&add.message),
            _ => None,
        };
        let anchor = addition.and_then(|key| {
            let exact = first_changes.get(&key).copied();
            let MessageKey::Basic(_, id) = key else {
                return exact;
            };
            exact
                .into_iter()
                .chain(first_changes.get(&MessageKey::CommonDeletion(id)).copied())
                .min()
        });
        // Synthetic additions have no independent pts. Move only before their
        // related edit/deletion and retain the SDK order of all other updates.
        let order = match anchor {
            Some(anchor) if anchor < index => (anchor, false),
            _ => (index, true),
        };
        ordered.push((order, entry));
    }
    ordered.sort_by_key(|(order, _)| *order);
    updates.extend(ordered.into_iter().map(|(_, entry)| entry));
}
