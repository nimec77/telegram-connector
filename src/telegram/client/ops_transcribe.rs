//! transcribe_audio operation and its TranscribeAudio RPC helper.
//!
//! Unit of `client` (LM-2).

use super::guard::require_found;
use super::*;

impl TelegramClient {
    pub(super) async fn transcribe_audio_impl(
        &self,
        channel_ref: &str,
        message_id: i32,
        timeout_secs: u32,
    ) -> Result<TranscriptionOutcome, Error> {
        use crate::telegram::transcription::{
            POLL_INTERVAL_SECS, ensure_transcribable, poll_until_complete,
        };

        validate_channel_identifier(channel_ref)?;

        // Resolve once; reuse the InputPeer for every poll (no repeated dialog walk).
        let peer = self.resolve_peer(channel_ref).await?;
        let peer_ref = peer_to_ref(&peer).await?;
        let input_peer: tl::enums::InputPeer = peer_ref.into();

        // Fetch the message to validate media type and read duration.
        let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
            self.client
                .get_messages_by_id(peer_ref, &[message_id])
                .await
                .map_err(|e| Error::TelegramApi(format!("Failed to get message: {}", e)))
        })
        .await?;
        let msg = require_found(
            messages.into_iter().next().flatten(),
            channel_ref,
            message_id,
        )?;
        let media = msg.media().ok_or_else(|| Error::NotTranscribable {
            media_type: "none".to_string(),
        })?;
        let media_type = convert_media_to_type(&media);
        ensure_transcribable(media_type)?;
        let duration_seconds = extract_audio_duration(&media);

        // Initial transcribeAudio call.
        let initial = self
            .invoke_transcribe(input_peer.clone(), message_id)
            .await?;

        // Poll (re-invoke) until complete or timeout.
        let (final_state, partial) = poll_until_complete(
            initial,
            StdDuration::from_secs(timeout_secs as u64),
            StdDuration::from_secs(POLL_INTERVAL_SECS),
            || {
                let peer = input_peer.clone();
                async move { self.invoke_transcribe(peer, message_id).await }
            },
        )
        .await;

        Ok(TranscriptionOutcome {
            text: final_state.text,
            partial,
            media_type,
            duration_seconds,
        })
    }

    /// Invoke `messages.transcribeAudio` once and parse the result into a
    /// [`TranscriptionState`]. Bounded by the history timeout budget.
    async fn invoke_transcribe(
        &self,
        peer: tl::enums::InputPeer,
        msg_id: i32,
    ) -> Result<TranscriptionState, Error> {
        use crate::telegram::transcription::map_transcribe_rpc_error;

        let request = tl::functions::messages::TranscribeAudio { peer, msg_id };
        let result = with_timeout("transcribe_audio", self.timeouts.history_secs, async {
            self.client
                .invoke(&request)
                .await
                .map_err(map_transcribe_rpc_error)
        })
        .await?;

        let tl::enums::messages::TranscribedAudio::Audio(t) = result;
        Ok(TranscriptionState {
            transcription_id: t.transcription_id,
            text: t.text,
            pending: t.pending,
        })
    }
}
