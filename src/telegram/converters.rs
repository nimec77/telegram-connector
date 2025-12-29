//! Type conversion helpers for grammers types to our domain types

use crate::telegram::types::{
    Channel, ChannelId, ChannelName, MediaType, Message, MessageId, UserId, Username,
};

/// Convert grammers Peer to our Channel type
pub fn convert_peer_to_channel(peer: &grammers_client::types::Peer) -> Option<Channel> {
    use grammers_client::types::Peer;

    match peer {
        Peer::Channel(ch) => {
            let id = ChannelId::new(ch.bare_id()).ok()?;
            let name = ChannelName::new(ch.title()).ok()?;
            let username = ch
                .username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("unknown").unwrap());

            Some(Channel {
                id,
                name,
                username,
                description: None, // Not available from basic chat info
                member_count: 0,   // Would need additional API call
                is_verified: ch.raw.verified,
                is_public: ch.username().is_some(),
                is_subscribed: true, // We're iterating our dialogs, so we're subscribed
                last_message_date: None,
            })
        }
        Peer::Group(g) => {
            // Include groups as they behave like channels for our purposes
            let id = ChannelId::new(g.id().bare_id()).ok()?;
            let name = ChannelName::new(g.title().unwrap_or("Unknown")).ok()?;
            let username = g
                .username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("group").unwrap());

            Some(Channel {
                id,
                name,
                username,
                description: None,
                member_count: 0,
                is_verified: false,
                is_public: g.username().is_some(),
                is_subscribed: true,
                last_message_date: None,
            })
        }
        _ => None, // Skip users
    }
}

/// Convert grammers Message to our Message type
pub fn convert_message(
    msg: &grammers_client::types::Message,
    peer: &grammers_client::types::Peer,
) -> Option<Message> {
    use grammers_client::types::Peer;

    let (channel_id, channel_name, channel_username) = match peer {
        Peer::Channel(ch) => (
            ChannelId::new(ch.bare_id()).ok()?,
            ChannelName::new(ch.title()).ok()?,
            ch.username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("unknown").unwrap()),
        ),
        Peer::Group(g) => (
            ChannelId::new(g.id().bare_id()).ok()?,
            ChannelName::new(g.title().unwrap_or("Unknown")).ok()?,
            g.username()
                .and_then(|u| Username::new(u).ok())
                .unwrap_or_else(|| Username::new("group").unwrap()),
        ),
        Peer::User(u) => (
            ChannelId::new(u.bare_id()).ok()?,
            ChannelName::new(u.first_name().unwrap_or("User")).ok()?,
            u.username()
                .and_then(|un| Username::new(un).ok())
                .unwrap_or_else(|| Username::new("user").unwrap()),
        ),
    };

    let message_id = MessageId::new(msg.id() as i64).ok()?;

    // Get sender info
    // msg.sender() returns Result<&Peer, Option<PeerRef>> in newer grammers versions
    let (sender_id, sender_name) = match msg.sender() {
        Ok(sender) => {
            let id = UserId::new(sender.id().bare_id()).ok();
            let name = sender.name().map(|s: &str| s.to_string());
            (id, name)
        }
        Err(_) => (None, None),
    };

    // Check for media
    let (has_media, media_type) = if msg.media().is_some() {
        (true, MediaType::Document) // Default to document
    } else {
        (false, MediaType::None)
    };

    Some(Message {
        id: message_id,
        channel_id,
        channel_name,
        channel_username,
        text: msg.text().to_string(),
        timestamp: msg.date(),
        sender_id,
        sender_name,
        has_media,
        media_type,
    })
}
