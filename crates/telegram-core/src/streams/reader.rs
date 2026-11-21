//! Drive the pinned SDK's ordered message boxes on the account sender's native receiver.

use super::subscription::Shared;
use super::{ChannelPost, ChannelPostMedia, InterruptionReason, UpdateObserver};
use crate::TelegramClient;
use crate::telegram::converters::{
    convert_media_to_type, extract_audio_info, extract_document_info, extract_video_info,
    timestamp_from_raw,
};
use crate::telegram::types::{ChannelId, MediaType, MessageId};
use futures::StreamExt;
use grammers_client::media::Media;
use grammers_client::peer::{Peer, User};
use grammers_client::{InvocationError, tl};
use grammers_session::Session;
use grammers_session::types::{PeerId, PeerInfo, UpdateState, UpdatesState};
use grammers_session::updates::{MessageBoxes, PrematureEndReason, UpdateAndPeers, UpdatesLike};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout, timeout_at};
use tracing::instrument::WithSubscriber as _;

const MAX_CONSECUTIVE_DIFFERENCES: usize = 8;
const GROUP_CHECKPOINT_RETRY: Duration = Duration::from_secs(60);
const GROUP_BOOTSTRAP_BUDGET: Duration = Duration::from_secs(5);
const GROUP_BOOTSTRAP_CONCURRENCY: usize = 4;

mod recovered;
use recovered::{apply_channel_batch, apply_common_batch};

pub(super) async fn run(
    client: Arc<TelegramClient>,
    updates: mpsc::Receiver<UpdatesLike>,
    shared: Arc<Shared>,
    maintenance_timeout: Duration,
    observer: Option<Arc<dyn UpdateObserver>>,
) {
    // Upstream sequencer/retry traces may carry raw updates, peers, or requests.
    // Request/queue counters are recorded separately without their contents.
    async {
        if let Err(failure) = read(
            &client,
            updates,
            &shared,
            maintenance_timeout,
            observer.as_deref(),
        )
        .await
        {
            let _ = report_gap(
                observer.as_deref(),
                None,
                matches!(failure, ReadFailure::Journal),
                maintenance_timeout,
            )
            .await;
        }
    }
    .with_subscriber(tracing::subscriber::NoSubscriber::default())
    .await;
}

#[derive(Debug)]
enum ReadFailure {
    Upstream,
    Journal,
}

impl From<()> for ReadFailure {
    fn from((): ()) -> Self {
        Self::Upstream
    }
}

async fn read(
    client: &TelegramClient,
    mut updates: mpsc::Receiver<UpdatesLike>,
    shared: &Shared,
    maintenance_timeout: Duration,
    observer: Option<&dyn UpdateObserver>,
) -> Result<(), ReadFailure> {
    let (mut boxes, mut missing_channels) =
        initial_boxes(client, shared, observer, maintenance_timeout).await?;
    let mut retry_channels_at = Instant::now() + GROUP_CHECKPOINT_RETRY;
    let mut recovering = observer.is_some();
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
                if observer.is_none() {
                    return Err(ReadFailure::Upstream);
                }
                // No difference is available for the common box. Establish a fresh
                // boundary after committing the coverage gap, retaining channel pts.
                report_gap(observer, None, false, maintenance_timeout).await?;
                let fresh = current_boxes(client, shared, maintenance_timeout).await?;
                let mut previous = boxes.session_state();
                if let Some(observer) = observer {
                    previous
                        .channels
                        .retain(|channel| observer.channels().contains(&channel.id));
                }
                boxes = MessageBoxes::load(UpdatesState {
                    channels: previous.channels,
                    ..fresh.session_state()
                });
                if let Some(observer) = observer {
                    save_checkpoint(
                        observer,
                        client.session(),
                        boxes.session_state(),
                        maintenance_timeout,
                    )
                    .await?;
                }
                differences = None;
                continue;
            }
            Some(apply_common_batch(
                &mut boxes,
                response,
                request.pts,
                observer.map(UpdateObserver::channels),
            ))
        } else if let Some(request) = boxes.get_channel_difference() {
            channel_difference(
                client,
                shared,
                &mut boxes,
                request,
                maintenance_timeout,
                &mut differences,
                observer,
            )
            .await?
        } else {
            differences = None;
            tokio::select! {
                _ = sleep_until(deadline.into()) => continue,
                _ = sleep_until(retry_channels_at), if !missing_channels.is_empty() => {
                    missing_channels = seed_channels(client, &mut boxes, &missing_channels, maintenance_timeout).await;
                    retry_channels_at = Instant::now() + GROUP_CHECKPOINT_RETRY;
                    if let Some(observer) = observer {
                        save_checkpoint(observer, client.session(), boxes.session_state(), maintenance_timeout).await?;
                    }
                    continue;
                }
                incoming = updates.recv() => {
                    let incoming = incoming.ok_or(())?;
                    if matches!(incoming, UpdatesLike::ConnectionClosed | UpdatesLike::MalformedUpdates) {
                        return Err(ReadFailure::Upstream);
                    }
                    let chats = socket_chats(&incoming);
                    observe_membership(shared, chats);
                    if let Some(observer) = observer
                        && !chats.is_empty()
                    {
                        timeout(maintenance_timeout, observer.observe_socket_membership(chats))
                            .await
                            .map_err(|_| ReadFailure::Journal)?
                            .map_err(|_| ReadFailure::Journal)?;
                    }
                    let mut incoming = adapt_short_channel_send(incoming);
                    if let Some(observer) = observer
                        && let Some(updates) = socket_updates(&mut incoming)
                    {
                        retain_channel_checkpoints(updates, &boxes.session_state(), observer.channels());
                    }
                    let batch = boxes.process_updates(incoming).ok();
                    if batch.is_some() {
                        recovering = false;
                    }
                    batch
                }
            }
        };
        if let Some(batch) = batch {
            if let Some(observer) = observer {
                checkpoint_observed_batch(
                    observer,
                    client.session(),
                    boxes.session_state(),
                    &batch,
                    recovering,
                    maintenance_timeout,
                )
                .await?;
            }
            observe_batch(client, shared, batch, maintenance_timeout).await?;
        }
    }
}

fn socket_updates(updates: &mut UpdatesLike) -> Option<&mut [tl::enums::Update]> {
    let updates = match updates {
        UpdatesLike::Updates(updates) => updates,
        UpdatesLike::InvitedUsers(result) => &mut result.updates,
        UpdatesLike::ChatInviteJoinResult(result) => &mut result.updates,
        _ => return None,
    };
    match updates {
        tl::enums::Updates::Updates(result) => Some(&mut result.updates),
        tl::enums::Updates::Combined(result) => Some(&mut result.updates),
        tl::enums::Updates::UpdateShort(result) => Some(std::slice::from_mut(&mut result.update)),
        _ => None,
    }
}

fn retain_channel_checkpoints(
    updates: &mut [tl::enums::Update],
    state: &UpdatesState,
    channels: &[i64],
) {
    for update in updates {
        if let tl::enums::Update::ChannelTooLong(marker) = update
            && channels.contains(&marker.channel_id)
            && state
                .channels
                .iter()
                .any(|channel| channel.id == marker.channel_id)
        {
            // A server position is an instruction to recover, not an acknowledged
            // local position. Ask the message box to keep the already known pts.
            marker.pts = None;
        }
    }
}

async fn initial_boxes(
    client: &TelegramClient,
    shared: &Shared,
    observer: Option<&dyn UpdateObserver>,
    max_time: Duration,
) -> Result<(MessageBoxes, Vec<i64>), ReadFailure> {
    let Some(observer) = observer else {
        // A pristine message box implements catch_up=false for SSE alone.
        return Ok((current_boxes(client, shared, max_time).await?, Vec::new()));
    };
    let mut saved = timeout(max_time, client.session().updates_state())
        .await
        .map_err(|_| ReadFailure::Journal)?
        .map_err(|_| ReadFailure::Journal)?;
    saved
        .channels
        .retain(|channel| observer.channels().contains(&channel.id));
    let mut boxes = if saved.pts == 0 {
        let fresh = current_boxes(client, shared, max_time).await?;
        if saved.channels.is_empty() {
            fresh
        } else {
            MessageBoxes::load(UpdatesState {
                channels: saved.channels,
                ..fresh.session_state()
            })
        }
    } else {
        MessageBoxes::load(saved)
    };
    let missing_channels = seed_channels(client, &mut boxes, observer.channels(), max_time).await;
    // Publish fresh streams only after the Telegram start positions have been sampled.
    timeout(max_time, observer.started())
        .await
        .map_err(|_| ReadFailure::Journal)?
        .map_err(|_| ReadFailure::Journal)?;
    for &id in &missing_channels {
        report_gap(Some(observer), Some(id), false, max_time).await?;
    }
    save_checkpoint(observer, client.session(), boxes.session_state(), max_time).await?;
    Ok((boxes, missing_channels))
}

async fn seed_channels(
    client: &TelegramClient,
    boxes: &mut MessageBoxes,
    channels: &[i64],
    max_time: Duration,
) -> Vec<i64> {
    let state = boxes.session_state();
    let mut missing = channels
        .iter()
        .copied()
        .filter(|id| !state.channels.iter().any(|channel| channel.id == *id))
        .collect::<Vec<_>>();
    let deadline = Instant::now() + max_time.min(GROUP_BOOTSTRAP_BUDGET);
    let mut probes = futures::stream::iter(missing.clone())
        .map(|id| async move { (id, client.joined_supergroup_pts(id).await) })
        .buffer_unordered(GROUP_BOOTSTRAP_CONCURRENCY);
    while let Ok(Some((id, result))) = timeout_at(deadline, probes.next()).await {
        if let Ok(pts) = result {
            missing.retain(|pending| *pending != id);
            if let Some(pts) = pts {
                boxes.try_set_channel_state(id, pts);
            }
        }
    }
    // Give peers that were behind a stalled probe window the first retry slots.
    let rotate_by = GROUP_BOOTSTRAP_CONCURRENCY.min(missing.len());
    missing.rotate_left(rotate_by);
    missing
}

async fn current_boxes(
    client: &TelegramClient,
    shared: &Shared,
    max_time: Duration,
) -> Result<MessageBoxes, ReadFailure> {
    let tl::enums::updates::State::State(state) = maintenance(
        client,
        shared,
        &tl::functions::updates::GetState {},
        max_time,
    )
    .await?
    .map_err(|_| ())?;
    let mut boxes = MessageBoxes::new();
    boxes.set_state(state);
    Ok(boxes)
}

async fn checkpoint_observed_batch(
    observer: &dyn UpdateObserver,
    session: &impl Session,
    state: UpdatesState,
    (updates, users, chats): &UpdateAndPeers,
    recovering: bool,
    max_time: Duration,
) -> Result<(), ReadFailure> {
    timeout(
        max_time,
        observer.observe(updates, users, chats, recovering),
    )
    .await
    .map_err(|_| ReadFailure::Journal)?
    .map_err(|_| ReadFailure::Journal)?;
    save_checkpoint(observer, session, state, max_time).await
}

async fn save_checkpoint(
    observer: &dyn UpdateObserver,
    session: &impl Session,
    mut state: UpdatesState,
    max_time: Duration,
) -> Result<(), ReadFailure> {
    state
        .channels
        .retain(|channel| observer.channels().contains(&channel.id));
    timeout(max_time, session.set_update_state(UpdateState::All(state)))
        .await
        .map_err(|_| ReadFailure::Journal)?
        .map_err(|_| ReadFailure::Journal)
}

async fn report_gap(
    observer: Option<&dyn UpdateObserver>,
    channel_id: Option<i64>,
    journal_failure: bool,
    max_time: Duration,
) -> Result<(), ReadFailure> {
    if let Some(observer) = observer
        && channel_id.is_none_or(|id| observer.channels().contains(&id))
    {
        timeout(max_time, observer.disconnected(channel_id, journal_failure))
            .await
            .map_err(|_| ReadFailure::Journal)?
            .map_err(|_| ReadFailure::Journal)?;
    }
    Ok(())
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
    observer: Option<&dyn UpdateObserver>,
) -> Result<Option<UpdateAndPeers>, ReadFailure> {
    let tl::enums::InputChannel::Channel(input) = &mut request.channel else {
        return Err(ReadFailure::Upstream);
    };
    let channel = ChannelId::new(input.channel_id).map_err(|_| ())?;
    if count_difference(differences, DifferenceTarget::Channel(input.channel_id)).is_err() {
        report_gap(observer, Some(channel.get()), false, max_time).await?;
        interrupt_channel(
            shared,
            boxes,
            channel,
            InterruptionReason::UpstreamDisconnected,
            observer.is_some_and(|observer| observer.channels().contains(&channel.get())),
        );
        return Ok(observer.map(|_| (Vec::new(), Vec::new(), Vec::new())));
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
        rebase_channel_after_gap(client, shared, boxes, channel, observer, max_time).await?;
        return Ok(observer.map(|_| (Vec::new(), Vec::new(), Vec::new())));
    };

    input.access_hash = auth.hash();
    let previous_pts = request.pts;
    request.limit = 100; // Hosted sessions represent users.
    let response = maintenance(client, shared, &request, max_time).await?;
    match response {
        Ok(tl::enums::updates::ChannelDifference::TooLong(_)) => {
            rebase_channel_after_gap(client, shared, boxes, channel, observer, max_time).await?;
            Ok(observer.map(|_| (Vec::new(), Vec::new(), Vec::new())))
        }
        Ok(response) => Ok(Some(apply_channel_batch(
            boxes,
            response,
            channel.get(),
            previous_pts,
            observer.map(UpdateObserver::channels),
        ))),
        Err(error) if error.is("CHANNEL_PRIVATE") || error.is("CHANNEL_INVALID") => {
            if let Some(observer) = observer
                && observer.channels().contains(&channel.get())
            {
                report_membership_loss(observer, channel, max_time).await?;
            }
            interrupt_channel(
                shared,
                boxes,
                channel,
                InterruptionReason::MembershipLost,
                false,
            );
            Ok(observer.map(|_| (Vec::new(), Vec::new(), Vec::new())))
        }
        Err(error) if error.is("PERSISTENT_TIMESTAMP_OUTDATED") => {
            rebase_channel_after_gap(client, shared, boxes, channel, observer, max_time).await?;
            Ok(observer.map(|_| (Vec::new(), Vec::new(), Vec::new())))
        }
        Err(InvocationError::Rpc(rpc)) if rpc.code == 500 => {
            report_gap(observer, Some(channel.get()), false, max_time).await?;
            interrupt_channel(
                shared,
                boxes,
                channel,
                InterruptionReason::UpstreamDisconnected,
                observer.is_some_and(|observer| observer.channels().contains(&channel.get())),
            );
            Ok(observer.map(|_| (Vec::new(), Vec::new(), Vec::new())))
        }
        Err(_) => Err(ReadFailure::Upstream),
    }
}

async fn rebase_channel_after_gap(
    client: &TelegramClient,
    shared: &Shared,
    boxes: &mut MessageBoxes,
    channel: ChannelId,
    observer: Option<&dyn UpdateObserver>,
    max_time: Duration,
) -> Result<(), ReadFailure> {
    let mut retry_later = false;
    let mut next_pts = None;
    if let Some(observer) = observer
        && observer.channels().contains(&channel.get())
    {
        match client.joined_supergroup_pts(channel.get()).await {
            Ok(None) => {
                report_membership_loss(observer, channel, max_time).await?;
                interrupt_channel(
                    shared,
                    boxes,
                    channel,
                    InterruptionReason::MembershipLost,
                    false,
                );
                return Ok(());
            }
            Ok(Some(pts)) => next_pts = Some(pts),
            Err(_) => retry_later = true,
        }
        report_gap(Some(observer), Some(channel.get()), false, max_time).await?;
    }
    interrupt_channel(
        shared,
        boxes,
        channel,
        InterruptionReason::UpstreamDisconnected,
        retry_later,
    );
    if let Some(pts) = next_pts {
        boxes.try_set_channel_state(channel.get(), pts);
    }
    Ok(())
}

async fn report_membership_loss(
    observer: &dyn UpdateObserver,
    channel: ChannelId,
    max_time: Duration,
) -> Result<(), ReadFailure> {
    timeout(max_time, observer.membership_lost(channel.get()))
        .await
        .map_err(|_| ReadFailure::Journal)?
        .map_err(|_| ReadFailure::Journal)
}

fn interrupt_channel(
    shared: &Shared,
    boxes: &mut MessageBoxes,
    channel: ChannelId,
    reason: InterruptionReason,
    retry_later: bool,
) {
    // A transient failure keeps the journal's channel checkpoint for periodic
    // recovery. Live subscribers still have to establish a new channel head.
    let end = if retry_later {
        PrematureEndReason::TemporaryServerIssues
    } else {
        PrematureEndReason::Banned
    };
    boxes.end_channel_difference(end);
    shared.interrupt_channel(channel, reason);
}

fn socket_chats(updates: &UpdatesLike) -> &[tl::enums::Chat] {
    match updates {
        UpdatesLike::Updates(tl::enums::Updates::Updates(envelope)) => &envelope.chats,
        UpdatesLike::Updates(tl::enums::Updates::Combined(envelope)) => &envelope.chats,
        _ => &[],
    }
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
        from_id: request.send_as.as_ref().and_then(input_peer_id),
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
        reply_to: request.reply_to.map(short_reply),
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

fn short_reply(reply: tl::enums::InputReplyTo) -> tl::enums::MessageReplyHeader {
    let (message_id, peer_id) = match reply {
        tl::enums::InputReplyTo::Message(message) if message.monoforum_peer_id.is_none() => {
            let peer = message.reply_to_peer_id.as_ref().and_then(input_peer_id);
            let resolvable = message.reply_to_peer_id.is_none() || peer.is_some();
            (resolvable.then_some(message.reply_to_msg_id), peer)
        }
        _ => (None, None),
    };
    tl::types::MessageReplyHeader {
        reply_to_scheduled: false,
        forum_topic: false,
        quote: false,
        reply_to_ephemeral: false,
        reply_to_msg_id: message_id,
        reply_to_peer_id: peer_id,
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

fn input_peer_id(peer: &tl::enums::InputPeer) -> Option<tl::enums::Peer> {
    let peer = match peer {
        tl::enums::InputPeer::Chat(peer) => tl::types::PeerChat {
            chat_id: peer.chat_id,
        }
        .into(),
        tl::enums::InputPeer::User(peer) => tl::types::PeerUser {
            user_id: peer.user_id,
        }
        .into(),
        tl::enums::InputPeer::UserFromMessage(peer) => tl::types::PeerUser {
            user_id: peer.user_id,
        }
        .into(),
        tl::enums::InputPeer::Channel(peer) => tl::types::PeerChannel {
            channel_id: peer.channel_id,
        }
        .into(),
        tl::enums::InputPeer::ChannelFromMessage(peer) => tl::types::PeerChannel {
            channel_id: peer.channel_id,
        }
        .into(),
        tl::enums::InputPeer::PeerSelf | tl::enums::InputPeer::Empty => return None,
    };
    Some(peer)
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod tests;
