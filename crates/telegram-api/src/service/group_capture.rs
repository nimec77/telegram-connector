//! Convert account-ordered Telegram updates to durable changes for enabled groups.

use super::group_journal::{GroupJournal, PendingChange};
use super::storage::StorageError;
use crate::contracts::groups::{Author, AuthorKind, Change, GapReason, GroupRef, ReplyKind};
use grammers_client::tl;
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use telegram_core::UtcTimestamp;
use telegram_core::error::Error;
use telegram_core::streams::{UpdateObserver, UpdatePosition};
use tokio::sync::{Mutex, Notify};
use tokio::time::Instant;

const MAX_EXCLUDED_MESSAGES: usize = 4096;
const MEMBERSHIP_INTERVAL: Duration = Duration::from_secs(60);

pub(super) struct Capture {
    journal: Arc<GroupJournal>,
    generation: String,
    configured: HashSet<GroupRef>,
    initially_joined: HashSet<GroupRef>,
    channels: Vec<i64>,
    state: Mutex<CaptureState>,
    monitoring: AtomicBool,
    changed: Arc<Notify>,
    pending_journal_gap: Arc<AtomicBool>,
}

struct CaptureState {
    // Fresh streams skip the account checkpoint's startup catch-up.
    active: HashMap<GroupRef, bool>,
    gaps_before_rejoin: HashSet<GroupRef>,
    pending_suspensions: HashMap<GroupRef, GapReason>,
    excluded: ExcludedMessages,
    membership_epoch: u64,
    last_membership_probe: Instant,
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
enum MessageNamespace {
    Common,
    Supergroup(i64),
}

#[derive(Default)]
struct ExcludedMessages {
    ids: HashSet<(MessageNamespace, i32)>,
    order: VecDeque<(MessageNamespace, i32)>,
}

impl ExcludedMessages {
    fn remember(&mut self, namespace: MessageNamespace, id: i32) {
        let key = (namespace, id);
        if self.ids.insert(key) {
            self.order.push_back(key);
        }
        if self.order.len() > MAX_EXCLUDED_MESSAGES
            && let Some(expired) = self.order.pop_front()
        {
            self.ids.remove(&expired);
        }
    }

    fn contains(&self, namespace: MessageNamespace, id: i32) -> bool {
        self.ids.contains(&(namespace, id))
    }
}

#[derive(Default)]
struct Batch {
    groups: HashMap<GroupRef, Vec<PendingChange>>,
    known_text: HashMap<(GroupRef, i32), bool>,
    original_adds: HashSet<(GroupRef, i32)>,
}

impl Batch {
    fn message(&mut self, group: GroupRef, id: i32, source: Option<String>, change: Change) {
        if matches!(change, Change::Added { .. }) {
            self.original_adds.insert((group, id));
        }
        self.known_text
            .insert((group, id), !matches!(change, Change::Deleted { .. }));
        self.push(group, source, change);
    }

    fn push(&mut self, group: GroupRef, source: Option<String>, change: Change) {
        self.groups
            .entry(group)
            .or_default()
            .push(PendingChange { source, change });
    }
}

impl Capture {
    pub(super) fn new(
        journal: Arc<GroupJournal>,
        generation: String,
        configured: HashSet<GroupRef>,
        initially_joined: HashSet<GroupRef>,
        changed: Arc<Notify>,
        pending_journal_gap: Arc<AtomicBool>,
    ) -> Self {
        let channels = configured
            .iter()
            .filter_map(|group| match group {
                GroupRef::Supergroup(id) => Some(*id),
                GroupRef::Chat(_) => None,
            })
            .collect();
        let monitoring = AtomicBool::new(!initially_joined.is_empty());
        Self {
            journal,
            generation,
            configured,
            initially_joined,
            channels,
            state: Mutex::new(CaptureState {
                active: HashMap::new(),
                gaps_before_rejoin: HashSet::new(),
                pending_suspensions: HashMap::new(),
                excluded: ExcludedMessages::default(),
                membership_epoch: 0,
                last_membership_probe: Instant::now(),
            }),
            monitoring,
            changed,
            pending_journal_gap,
        }
    }

    pub(super) fn is_monitoring(&self) -> bool {
        self.monitoring.load(Ordering::Acquire)
    }

    pub(super) async fn refresh_memberships(
        &self,
        fetch: impl Future<Output = HashSet<GroupRef>>,
    ) -> Result<(), Error> {
        let epoch = {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            if now.duration_since(state.last_membership_probe) < MEMBERSHIP_INTERVAL {
                return Ok(());
            }
            state.last_membership_probe = now;
            state.membership_epoch
        };
        let joined = fetch.await;
        let mut state = self.state.lock().await;
        if state.membership_epoch != epoch {
            return Ok(());
        }
        let suspended: Vec<_> = state
            .active
            .keys()
            .filter(|group| !joined.contains(group))
            .copied()
            .collect();
        for group in suspended {
            self.suspend(&mut state, group);
        }
        self.flush_suspensions(&mut state).await?;
        for &group in self.configured.intersection(&joined) {
            self.activate(&mut state, group).await?;
        }
        Ok(())
    }

    async fn append(&self, group: GroupRef, changes: Vec<PendingChange>) -> Result<(), Error> {
        self.journal
            .append(&self.generation, &group.to_string(), changes)
            .await
            .map_err(journal_error)?;
        self.changed.notify_waiters();
        Ok(())
    }

    async fn gap(&self, group: GroupRef, reason: GapReason) -> Result<(), Error> {
        self.append(
            group,
            vec![PendingChange {
                source: None,
                change: Change::Gap { reason },
            }],
        )
        .await
    }

    async fn enable(&self, group: GroupRef) -> Result<bool, Error> {
        self.journal
            .enable(&self.generation, &group.to_string())
            .await
            .map_err(journal_error)
    }

    async fn observe_membership(
        &self,
        state: &mut CaptureState,
        chats: &[tl::enums::Chat],
    ) -> Result<(), Error> {
        let mut observations = HashMap::new();
        for chat in chats {
            if let Some((group, joined)) = membership(chat)
                && self.configured.contains(&group)
            {
                observations
                    .entry(group)
                    .and_modify(|confirmed| *confirmed &= joined)
                    .or_insert(joined);
            }
        }
        for (&group, _) in observations.iter().filter(|(_, joined)| !**joined) {
            self.suspend(state, group);
        }
        self.flush_suspensions(state).await?;
        for (group, _) in observations.into_iter().filter(|(_, joined)| *joined) {
            self.activate(state, group).await?;
        }
        Ok(())
    }

    async fn activate(&self, state: &mut CaptureState, group: GroupRef) -> Result<(), Error> {
        if state.active.contains_key(&group) {
            return Ok(());
        }
        let fresh = self.enable(group).await?;
        if !fresh && !state.gaps_before_rejoin.contains(&group) {
            self.gap(group, GapReason::UpstreamDisconnect).await?;
        }
        state.gaps_before_rejoin.remove(&group);
        state.active.insert(group, true);
        state.membership_epoch = state.membership_epoch.wrapping_add(1);
        self.monitoring.store(true, Ordering::Release);
        Ok(())
    }

    fn suspend(&self, state: &mut CaptureState, group: GroupRef) {
        if !self.configured.contains(&group) {
            return;
        }
        state.membership_epoch = state.membership_epoch.wrapping_add(1);
        if state.active.remove(&group).is_some() {
            state
                .pending_suspensions
                .insert(group, GapReason::UpstreamDisconnect);
            self.monitoring
                .store(!state.active.is_empty(), Ordering::Release);
        }
    }

    async fn flush_suspensions(&self, state: &mut CaptureState) -> Result<(), Error> {
        while let Some((&group, &reason)) = state.pending_suspensions.iter().next() {
            if let Err(error) = self.gap(group, reason).await {
                self.pending_journal_gap.store(true, Ordering::Release);
                state
                    .pending_suspensions
                    .insert(group, GapReason::JournalFailure);
                return Err(error);
            }
            state.pending_suspensions.remove(&group);
            state.gaps_before_rejoin.insert(group);
        }
        Ok(())
    }

    async fn record_deletions(
        &self,
        state: &CaptureState,
        batch: &mut Batch,
        namespace: MessageNamespace,
        ids: &[i32],
        pts: i32,
        recovering: bool,
    ) -> Result<(), Error> {
        let groups: Vec<_> = state
            .active
            .iter()
            .filter(|(group, fresh)| {
                !(recovering && **fresh)
                    && match (namespace, group) {
                        (MessageNamespace::Common, GroupRef::Chat(_)) => true,
                        (MessageNamespace::Supergroup(id), GroupRef::Supergroup(group)) => {
                            id == *group
                        }
                        _ => false,
                    }
            })
            .map(|(&group, _)| (group, group.to_string()))
            .collect();
        if groups.is_empty() {
            return Ok(());
        }
        for &id in ids.iter().filter(|&&id| id > 0) {
            let message_id = id.to_string();
            let source = ordered_source("deleted", pts, id);
            let mut matched = Vec::new();
            let mut replayed = false;
            for (group, reference) in &groups {
                if let Some(source) = source.as_deref()
                    && self
                        .journal
                        .known_source(&self.generation, reference, source)
                        .await
                        .map_err(journal_error)?
                {
                    replayed = true;
                    break;
                }
                let known = match batch.known_text.get(&(*group, id)) {
                    Some(known) => *known,
                    None => self
                        .journal
                        .known_message(&self.generation, reference, &message_id)
                        .await
                        .map_err(journal_error)?,
                };
                if known {
                    matched.push(*group);
                }
            }
            if replayed {
                continue;
            }
            if let [group] = matched.as_slice() {
                batch.message(
                    *group,
                    id,
                    source,
                    Change::Deleted {
                        message_id,
                        deleted_at: String::new(),
                    },
                );
            } else if !matched.is_empty()
                || (!state.excluded.contains(namespace, id)
                    && !groups
                        .iter()
                        .any(|(group, _)| batch.known_text.get(&(*group, id)) == Some(&false)))
            {
                let possible = if matched.is_empty() {
                    groups.iter().map(|(group, _)| *group).collect()
                } else {
                    matched
                };
                for group in possible {
                    batch.push(
                        group,
                        ordered_source("ambiguous_deletion", pts, id),
                        Change::Gap {
                            reason: GapReason::AmbiguousUpdate,
                        },
                    );
                }
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl UpdateObserver for Capture {
    fn channels(&self) -> &[i64] {
        &self.channels
    }

    async fn started(&self) -> Result<(), Error> {
        let mut state = self.state.lock().await;
        let journal_failed = self.pending_journal_gap.load(Ordering::Acquire);
        for &group in &self.initially_joined {
            let fresh = self.enable(group).await?;
            if !fresh || journal_failed {
                let reason = if journal_failed {
                    GapReason::JournalFailure
                } else {
                    GapReason::UpstreamDisconnect
                };
                self.gap(group, reason).await?;
            }
            state.active.insert(group, fresh);
        }
        let monitoring = !state.active.is_empty();
        state.membership_epoch = state.membership_epoch.wrapping_add(1);
        self.monitoring.store(monitoring, Ordering::Release);
        if monitoring {
            self.pending_journal_gap.store(false, Ordering::Release);
        }
        Ok(())
    }

    async fn observe_socket_membership(&self, chats: &[tl::enums::Chat]) -> Result<(), Error> {
        let mut state = self.state.lock().await;
        for (group, _) in chats
            .iter()
            .filter_map(membership)
            .filter(|(_, joined)| !joined)
        {
            self.suspend(&mut state, group);
        }
        self.flush_suspensions(&mut state).await
    }

    async fn observe(
        &self,
        updates: &[(tl::enums::Update, UpdatePosition)],
        users: &[tl::enums::User],
        chats: &[tl::enums::Chat],
        recovering: bool,
    ) -> Result<(), Error> {
        let mut state = self.state.lock().await;
        // A startup difference can contain older membership snapshots. The
        // account already checked current membership before starting capture.
        if !recovering {
            self.observe_membership(&mut state, chats).await?;
        }
        let mut batch = Batch::default();
        for (update, _) in updates {
            let (message, edit, pts) = match update {
                tl::enums::Update::NewMessage(update) => (&update.message, false, update.pts),
                tl::enums::Update::NewChannelMessage(update) => {
                    (&update.message, false, update.pts)
                }
                tl::enums::Update::EditMessage(update) => (&update.message, true, update.pts),
                tl::enums::Update::EditChannelMessage(update) => {
                    (&update.message, true, update.pts)
                }
                tl::enums::Update::DeleteMessages(update) => {
                    self.record_deletions(
                        &state,
                        &mut batch,
                        MessageNamespace::Common,
                        &update.messages,
                        update.pts,
                        recovering,
                    )
                    .await?;
                    continue;
                }
                tl::enums::Update::DeleteChannelMessages(update) => {
                    self.record_deletions(
                        &state,
                        &mut batch,
                        MessageNamespace::Supergroup(update.channel_id),
                        &update.messages,
                        update.pts,
                        recovering,
                    )
                    .await?;
                    continue;
                }
                _ => continue,
            };
            let id = message.id();
            if id <= 0 {
                continue;
            }
            if let tl::enums::Message::Message(message) = message
                && let Some(group) = group_peer(&message.peer_id)
                && state
                    .active
                    .get(&group)
                    .is_some_and(|fresh| !(recovering && *fresh))
                && let Some(change) = decode_message(message, edit, users, chats)
            {
                if !edit && pts == 0 && message.edit_date.is_some() {
                    // The SDK wraps difference snapshots as additions with no
                    // source PTS. An edited snapshot cannot prove the original text.
                    if !batch.original_adds.contains(&(group, id))
                        && !self
                            .journal
                            .known_source(
                                &self.generation,
                                &group.to_string(),
                                &format!("added:{id}"),
                            )
                            .await
                            .map_err(journal_error)?
                    {
                        batch.push(
                            group,
                            Some(format!("edited_snapshot:{id}")),
                            Change::Gap {
                                reason: GapReason::UpstreamDisconnect,
                            },
                        );
                    }
                    continue;
                }
                // Telegram keeps one message ID for the original even when a
                // difference returns it without a distinct source PTS.
                let source = if edit {
                    ordered_source("edited", pts, id)
                } else {
                    Some(format!("added:{id}"))
                };
                batch.message(group, id, source, change);
            } else if let Some(namespace) = message_namespace(message)
                && (namespace == MessageNamespace::Common
                    || matches!(namespace, MessageNamespace::Supergroup(id)
                        if self.configured.contains(&GroupRef::Supergroup(id))))
            {
                // Remember IDs only in process so filtered updates and their
                // bodies are never written to the journal.
                state.excluded.remember(namespace, id);
            }
        }
        for (group, changes) in batch.groups {
            self.append(group, changes).await?;
        }
        Ok(())
    }

    async fn disconnected(
        &self,
        channel_id: Option<i64>,
        journal_failure: bool,
    ) -> Result<(), Error> {
        let state = self.state.lock().await;
        let reason = if journal_failure {
            self.pending_journal_gap.store(true, Ordering::Release);
            GapReason::JournalFailure
        } else {
            GapReason::UpstreamDisconnect
        };
        for &group in state.active.keys() {
            if channel_id.is_none_or(|id| group == GroupRef::Supergroup(id)) {
                self.gap(group, reason).await?;
            }
        }
        // A failed startup may not have populated the set yet. Keep the
        // pending marker until the next successful startup.
        if journal_failure && !state.active.is_empty() && state.pending_suspensions.is_empty() {
            self.pending_journal_gap.store(false, Ordering::Release);
        }
        Ok(())
    }

    async fn membership_lost(&self, channel_id: i64) -> Result<(), Error> {
        let mut state = self.state.lock().await;
        self.suspend(&mut state, GroupRef::Supergroup(channel_id));
        self.flush_suspensions(&mut state).await
    }
}

fn membership(chat: &tl::enums::Chat) -> Option<(GroupRef, bool)> {
    match chat {
        tl::enums::Chat::Chat(chat) => Some((
            GroupRef::Chat(chat.id),
            !chat.left && !chat.deactivated && chat.migrated_to.is_none(),
        )),
        tl::enums::Chat::Forbidden(chat) => Some((GroupRef::Chat(chat.id), false)),
        tl::enums::Chat::Channel(channel) if !channel.min => Some((
            GroupRef::Supergroup(channel.id),
            !channel.left && channel.megagroup && !channel.broadcast && !channel.monoforum,
        )),
        tl::enums::Chat::ChannelForbidden(channel) => {
            Some((GroupRef::Supergroup(channel.id), false))
        }
        _ => None,
    }
}

fn group_peer(peer: &tl::enums::Peer) -> Option<GroupRef> {
    match peer {
        tl::enums::Peer::Chat(peer) => Some(GroupRef::Chat(peer.chat_id)),
        tl::enums::Peer::Channel(peer) => Some(GroupRef::Supergroup(peer.channel_id)),
        tl::enums::Peer::User(_) => None,
    }
}

fn message_namespace(message: &tl::enums::Message) -> Option<MessageNamespace> {
    let peer = match message {
        tl::enums::Message::Message(message) => &message.peer_id,
        tl::enums::Message::Service(message) => &message.peer_id,
        tl::enums::Message::Empty(message) => message.peer_id.as_ref()?,
    };
    Some(match peer {
        tl::enums::Peer::Channel(channel) => MessageNamespace::Supergroup(channel.channel_id),
        tl::enums::Peer::Chat(_) | tl::enums::Peer::User(_) => MessageNamespace::Common,
    })
}

fn decode_message(
    message: &tl::types::Message,
    edit: bool,
    users: &[tl::enums::User],
    chats: &[tl::enums::Chat],
) -> Option<Change> {
    if message.message.is_empty()
        || !matches!(message.media, None | Some(tl::enums::MessageMedia::Empty))
        || message.grouped_id.is_some()
    {
        return None;
    }
    let message_id = message.id.to_string();
    let change = if edit {
        Change::Edited {
            message_id,
            edited_at: timestamp(message.edit_date?)?,
            text: message.message.clone(),
        }
    } else {
        let (reply_kind, reply_to_message_id) = reply(message);
        Change::Added {
            message_id,
            posted_at: timestamp(message.date)?,
            author: author(message.from_id.as_ref(), users, chats),
            text: message.message.clone(),
            reply_kind,
            reply_to_message_id,
        }
    };
    Some(change)
}

fn timestamp(seconds: i32) -> Option<String> {
    UtcTimestamp::from_unix_seconds(i64::from(seconds))
        .ok()
        .map(UtcTimestamp::to_rfc3339_seconds)
}

fn reply(message: &tl::types::Message) -> (ReplyKind, Option<String>) {
    match &message.reply_to {
        None => (ReplyKind::None, None),
        Some(tl::enums::MessageReplyHeader::Header(reply))
            if !reply.reply_to_scheduled
                && !reply.reply_to_ephemeral
                && reply
                    .reply_to_peer_id
                    .as_ref()
                    .is_none_or(|peer| peer == &message.peer_id)
                && let Some(id) = reply.reply_to_msg_id.filter(|&id| id > 0) =>
        {
            (ReplyKind::Local, Some(id.to_string()))
        }
        Some(_) => (ReplyKind::Unavailable, None),
    }
}

fn author(
    author: Option<&tl::enums::Peer>,
    users: &[tl::enums::User],
    chats: &[tl::enums::Chat],
) -> Author {
    let mut identity = Author {
        kind: AuthorKind::Unknown,
        user_id: None,
        display_name: None,
        username: None,
    };
    match author {
        Some(tl::enums::Peer::User(peer)) if peer.user_id > 0 => {
            identity.user_id = Some(peer.user_id.to_string());
            if let Some(user) = users.iter().find_map(|user| match user {
                tl::enums::User::User(user) if user.id == peer.user_id => Some(user),
                _ => None,
            }) {
                let name = [user.first_name.as_deref(), user.last_name.as_deref()]
                    .into_iter()
                    .flatten()
                    .filter(|component| !component.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                identity.kind = if user.bot {
                    AuthorKind::Bot
                } else {
                    AuthorKind::User
                };
                identity.username.clone_from(&user.username);
                identity.display_name = if name.is_empty() {
                    user.username
                        .clone()
                        .or_else(|| (!user.bot).then(String::new))
                } else {
                    Some(name)
                };
            }
        }
        Some(peer @ (tl::enums::Peer::Chat(_) | tl::enums::Peer::Channel(_))) => {
            identity.kind = AuthorKind::Chat;
            for chat in chats {
                match (peer, chat) {
                    (tl::enums::Peer::Chat(peer), tl::enums::Chat::Chat(chat))
                        if peer.chat_id == chat.id =>
                    {
                        identity.display_name = Some(chat.title.clone());
                        break;
                    }
                    (tl::enums::Peer::Channel(peer), tl::enums::Chat::Channel(channel))
                        if peer.channel_id == channel.id =>
                    {
                        identity.display_name = Some(channel.title.clone());
                        identity.username.clone_from(&channel.username);
                        break;
                    }
                    _ => {}
                }
            }
        }
        Some(tl::enums::Peer::User(_)) | None => {}
    }
    identity
}

fn ordered_source(kind: &str, pts: i32, id: i32) -> Option<String> {
    (pts > 0).then(|| format!("{kind}:{pts}:{id}"))
}

fn journal_error(_: StorageError) -> Error {
    Error::Internal("Group journal unavailable")
}

#[cfg(test)]
#[path = "group_capture/tests.rs"]
mod tests;
