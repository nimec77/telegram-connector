//! Drive the pinned SDK's ordered message boxes on the account sender's native receiver.

use super::subscription::Shared;
use super::{ChannelPost, ChannelPostMedia, InterruptionReason};
use crate::TelegramClient;
use crate::telegram::converters::{
    convert_media_to_type, extract_audio_info, extract_document_info, extract_video_info,
    timestamp_from_raw,
};
use crate::telegram::types::{ChannelId, MediaType, MessageId};
use grammers_client::media::Media;
use grammers_client::peer::{Peer, User};
use grammers_client::{InvocationError, tl};
use grammers_session::Session;
use grammers_session::types::{PeerId, PeerInfo};
use grammers_session::updates::{MessageBoxes, PrematureEndReason, UpdateAndPeers, UpdatesLike};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, timeout};
use tracing::instrument::WithSubscriber as _;

const MAX_CONSECUTIVE_DIFFERENCES: usize = 8;

pub(super) async fn run(
    client: Arc<TelegramClient>,
    updates: mpsc::Receiver<UpdatesLike>,
    shared: Arc<Shared>,
    maintenance_timeout: Duration,
) {
    // Upstream sequencer/retry traces may carry raw updates, peers, or requests.
    // Request/queue counters are recorded separately without their contents.
    let _ = read(&client, updates, &shared, maintenance_timeout)
        .with_subscriber(tracing::subscriber::NoSubscriber::default())
        .await;
}

async fn read(
    client: &TelegramClient,
    mut updates: mpsc::Receiver<UpdatesLike>,
    shared: &Shared,
    maintenance_timeout: Duration,
) -> Result<(), ()> {
    // A pristine message box and the current state implement catch_up=false.
    // Complete initialization before any subscriber begins taking its heads.
    let mut boxes = MessageBoxes::new();
    let tl::enums::updates::State::State(state) = maintenance(
        client,
        shared,
        &tl::functions::updates::GetState {},
        maintenance_timeout,
    )
    .await?
    .map_err(|_| ())?;
    boxes.set_state(state);
    shared.mark_started();

    let mut differences = None;
    loop {
        let deadline = boxes.check_deadlines();
        let batch = if let Some(request) = boxes.get_difference() {
            count_difference(&mut differences, DifferenceTarget::Common)?;
            let response = maintenance(client, shared, &request, maintenance_timeout)
                .await?
                .map_err(|_| ())?;
            if matches!(response, tl::enums::updates::Difference::TooLong(_)) {
                return Err(());
            }
            Some(boxes.apply_difference(response))
        } else if let Some(request) = boxes.get_channel_difference() {
            channel_difference(
                client,
                shared,
                &mut boxes,
                request,
                maintenance_timeout,
                &mut differences,
            )
            .await?
        } else {
            differences = None;
            tokio::select! {
                _ = sleep_until(deadline.into()) => continue,
                incoming = updates.recv() => {
                    let incoming = incoming.ok_or(())?;
                    if matches!(incoming, UpdatesLike::ConnectionClosed | UpdatesLike::MalformedUpdates) {
                        return Err(());
                    }
                    observe_socket_membership(shared, &incoming);
                    boxes.process_updates(adapt_short_channel_send(incoming)).ok()
                }
            }
        };
        if let Some(batch) = batch {
            observe_batch(client, shared, batch, maintenance_timeout).await?;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DifferenceTarget {
    Common,
    Channel(i64),
}

fn count_difference(
    consecutive: &mut Option<(DifferenceTarget, usize)>,
    target: DifferenceTarget,
) -> Result<(), ()> {
    let (previous, pages) = consecutive.get_or_insert((target, 0));
    if *previous != target {
        (*previous, *pages) = (target, 0);
    }
    if *pages >= MAX_CONSECUTIVE_DIFFERENCES {
        return Err(());
    }
    *pages += 1;
    Ok(())
}

async fn maintenance<R: tl::RemoteCall>(
    client: &TelegramClient,
    shared: &Shared,
    request: &R,
    max_time: Duration,
) -> Result<Result<R::Return, InvocationError>, ()> {
    shared.count_maintenance_request();
    timeout(max_time, client.client().invoke(request))
        .await
        .map_err(|_| ())
}

async fn channel_difference(
    client: &TelegramClient,
    shared: &Shared,
    boxes: &mut MessageBoxes,
    mut request: tl::functions::updates::GetChannelDifference,
    max_time: Duration,
    differences: &mut Option<(DifferenceTarget, usize)>,
) -> Result<Option<UpdateAndPeers>, ()> {
    let tl::enums::InputChannel::Channel(input) = &mut request.channel else {
        return Err(());
    };
    let channel = ChannelId::new(input.channel_id).map_err(|_| ())?;
    if count_difference(differences, DifferenceTarget::Channel(input.channel_id)).is_err() {
        interrupt_channel(
            shared,
            boxes,
            channel,
            InterruptionReason::UpstreamDisconnected,
        );
        return Ok(None);
    }
    let peer_id = PeerId::channel(input.channel_id).ok_or(())?;
    let peer = timeout(max_time, client.session().peer(peer_id))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    let Some(PeerInfo::Channel {
        auth: Some(auth), ..
    }) = peer
    else {
        interrupt_channel(
            shared,
            boxes,
            channel,
            InterruptionReason::UpstreamDisconnected,
        );
        return Ok(None);
    };

    input.access_hash = auth.hash();
    request.limit = 100; // Hosted sessions represent users.
    let response = maintenance(client, shared, &request, max_time).await?;
    match response {
        Ok(tl::enums::updates::ChannelDifference::TooLong(_)) => {
            interrupt_channel(
                shared,
                boxes,
                channel,
                InterruptionReason::UpstreamDisconnected,
            );
            Ok(None)
        }
        Ok(mut response) => {
            if let tl::enums::updates::ChannelDifference::Difference(difference) = &mut response {
                difference.new_messages.sort_by_key(tl::enums::Message::id);
            }
            Ok(Some(boxes.apply_channel_difference(response)))
        }
        Err(error) if error.is("CHANNEL_PRIVATE") || error.is("CHANNEL_INVALID") => {
            interrupt_channel(shared, boxes, channel, InterruptionReason::MembershipLost);
            Ok(None)
        }
        Err(error)
            if error.is("PERSISTENT_TIMESTAMP_OUTDATED")
                || matches!(&error, InvocationError::Rpc(rpc) if rpc.code == 500) =>
        {
            interrupt_channel(
                shared,
                boxes,
                channel,
                InterruptionReason::UpstreamDisconnected,
            );
            Ok(None)
        }
        Err(_) => Err(()),
    }
}

fn interrupt_channel(
    shared: &Shared,
    boxes: &mut MessageBoxes,
    channel: ChannelId,
    reason: InterruptionReason,
) {
    // Discard unsafe continuity. A fresh subscriber captures a new head.
    boxes.end_channel_difference(PrematureEndReason::Banned);
    shared.interrupt_channel(channel, reason);
}

fn observe_socket_membership(shared: &Shared, updates: &UpdatesLike) {
    let chats = match updates {
        UpdatesLike::Updates(tl::enums::Updates::Updates(envelope)) => &envelope.chats,
        UpdatesLike::Updates(tl::enums::Updates::Combined(envelope)) => &envelope.chats,
        _ => return,
    };
    observe_membership(shared, chats);
}

fn observe_membership(shared: &Shared, chats: &[tl::enums::Chat]) {
    for chat in chats {
        if let Some((channel, false)) = super::broadcast_membership(chat) {
            shared.interrupt_channel(channel, InterruptionReason::MembershipLost);
        }
    }
}

async fn observe_batch(
    client: &TelegramClient,
    shared: &Shared,
    (updates, users, chats): UpdateAndPeers,
    max_time: Duration,
) -> Result<(), ()> {
    observe_membership(shared, &chats);
    // Populate the same SDK session cache used by the sender and by future
    // difference requests. Only full-authority peers are persisted.
    let known: Vec<_> = users
        .into_iter()
        .map(|user| Peer::User(User::from_raw(client.client(), user)))
        .chain(
            chats
                .into_iter()
                .map(|chat| Peer::from_raw(client.client(), chat)),
        )
        .map(|peer| PeerInfo::from(&peer))
        .filter(|info| info.auth().is_some())
        .collect();
    if !known.is_empty() {
        timeout(max_time, client.session().cache_peers(known))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
    }
    for (update, _) in updates {
        if let Some(post) = decode_post(update)
            && shared.interested(post.channel_id)
        {
            shared.publish(post);
        }
    }
    Ok(())
}

pub(super) fn decode_post(update: tl::enums::Update) -> Option<ChannelPost> {
    let tl::enums::Update::NewChannelMessage(update) = update else {
        return None;
    };
    let published_at = timestamp_from_raw(&update.message)?;
    let tl::enums::Message::Message(raw) = update.message else {
        return None;
    };
    let tl::enums::Peer::Channel(peer) = raw.peer_id else {
        return None;
    };
    if !raw.post {
        return None;
    }
    Some(ChannelPost {
        channel_id: ChannelId::new(peer.channel_id).ok()?,
        message_id: MessageId::new(i64::from(raw.id)).ok()?,
        published_at,
        text: raw.message,
        media: raw
            .media
            .and_then(Media::from_raw)
            .as_ref()
            .and_then(media_metadata),
        album_id: raw.grouped_id,
    })
}

fn media_metadata(media: &Media) -> Option<ChannelPostMedia> {
    let kind = convert_media_to_type(media);
    if kind == MediaType::None {
        return None;
    }
    let video = extract_video_info(media);
    let audio = extract_audio_info(media);
    let file_size_bytes = video
        .as_ref()
        .map(|info| info.file_size_bytes)
        .or_else(|| audio.as_ref().map(|info| info.file_size_bytes))
        .or_else(|| extract_document_info(media).map(|info| info.file_size_bytes));
    Some(ChannelPostMedia {
        kind,
        file_size_bytes,
        duration_seconds: video
            .as_ref()
            .map(|info| info.duration_seconds)
            .or_else(|| audio.map(|info| info.duration_seconds)),
        width: video.as_ref().map(|info| info.width),
        height: video.as_ref().map(|info| info.height),
    })
}

pub(super) fn adapt_short_channel_send(updates: UpdatesLike) -> UpdatesLike {
    let UpdatesLike::ShortSentMessage { request, update } = updates else {
        return updates;
    };
    let tl::enums::InputPeer::Channel(peer) = &request.peer else {
        return UpdatesLike::ShortSentMessage { request, update };
    };
    // The pinned SDK converts all short sends into NewMessage, then asserts
    // that NewMessage belongs to the common (non-channel) box. A channel short
    // send must go through the channel box with its original pts instead.
    let message = tl::types::Message {
        out: update.out,
        mentioned: false,
        media_unread: false,
        silent: request.silent,
        post: true,
        from_scheduled: false,
        legacy: false,
        edit_hide: false,
        pinned: false,
        noforwards: request.noforwards,
        invert_media: request.invert_media,
        offline: false,
        video_processing_pending: false,
        paid_suggested_post_stars: false,
        paid_suggested_post_ton: false,
        id: update.id,
        from_id: None,
        from_boosts_applied: None,
        from_rank: None,
        peer_id: tl::types::PeerChannel {
            channel_id: peer.channel_id,
        }
        .into(),
        saved_peer_id: None,
        fwd_from: None,
        via_bot_id: None,
        via_business_bot_id: None,
        guestchat_via_from: None,
        reply_to: None,
        date: update.date,
        message: request.message,
        media: update.media,
        reply_markup: request.reply_markup,
        entities: update.entities.or(request.entities),
        views: None,
        forwards: None,
        replies: None,
        edit_date: None,
        post_author: None,
        grouped_id: None,
        reactions: None,
        restriction_reason: None,
        ttl_period: update.ttl_period,
        quick_reply_shortcut_id: None,
        effect: request.effect,
        factcheck: None,
        report_delivery_until_date: None,
        paid_message_stars: None,
        suggested_post: None,
        schedule_repeat_period: None,
        summary_from_language: None,
        rich_message: None,
    };
    UpdatesLike::Updates(tl::enums::Updates::UpdateShort(tl::types::UpdateShort {
        update: tl::types::UpdateNewChannelMessage {
            message: message.into(),
            pts: update.pts,
            pts_count: update.pts_count,
        }
        .into(),
        date: update.date,
    }))
}

#[cfg(test)]
mod tests {
    use super::{DifferenceTarget, MAX_CONSECUTIVE_DIFFERENCES, count_difference};

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
}
