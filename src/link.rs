use crate::telegram::types::{ChannelId, MessageId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageLink {
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub https_link: String,
    pub tg_protocol_link: String,
}

impl MessageLink {
    pub fn new(_channel_id: ChannelId, _message_id: MessageId) -> Self {
        todo!("Generate message links - Phase 6")
    }
}
