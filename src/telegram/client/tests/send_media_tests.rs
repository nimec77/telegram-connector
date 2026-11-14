use super::resolve_recipient::from_peer;
use super::send_media::{prepare_media_request, validate_before_upload};
use super::send_targets::ValidatedTargets;
use crate::sending::staging::StagingStore;
use crate::sending::test_support::Fixture;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{
    PreparedMediaSend, RecipientSpec, SendMediaKind, SendOptions,
};
use grammers_client::media::Uploaded;
use grammers_client::peer::Peer;
use grammers_client::tl;
use std::sync::Arc;

#[tokio::test]
async fn sending_media_requests_have_exact_kind_attributes_and_no_unrequested_mutations() {
    let fixture = Fixture::new();
    let audio = b"\xff\xfb\x90\x00abcdefgh";
    std::fs::write(fixture.source.join("audio.mp3"), audio).expect("MP3 header");
    let mut voice = vec![0; 27];
    voice[..4].copy_from_slice(b"OggS");
    voice[5] = 2;
    voice[26] = 1;
    voice.push(19);
    voice.extend_from_slice(b"OpusHead\x01\x01\0\0\x80\xbb\0\0\0\0\0");
    std::fs::write(fixture.source.join("voice.ogg"), voice).expect("Opus header");
    std::fs::write(
        fixture.source.join("video.mp4"),
        b"\0\0\0\x10ftypmp42\0\0\0\0",
    )
    .expect("MP4 header");
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut raw = crate::test_helpers::raw_tl_channel(91, "private", None);
    raw.date = 1_700_000_000;
    let peer = from_peer(&Peer::from_raw(client.client(), raw.into()))
        .await
        .expect("recipient");
    for (name, kind) in [
        ("image.jpg", SendMediaKind::Image),
        (
            "audio.mp3",
            SendMediaKind::Audio {
                duration_seconds: 7,
                title: Some("title".into()),
                performer: Some("artist".into()),
            },
        ),
        (
            "voice.ogg",
            SendMediaKind::Voice {
                duration_seconds: 8,
            },
        ),
        (
            "video.mp4",
            SendMediaKind::Video {
                duration_seconds: 12.5,
                width: 1280,
                height: 720,
            },
        ),
    ] {
        let store = StagingStore::try_new(&fixture.config)
            .await
            .expect("fresh store");
        let staged = store
            .stage(
                &fixture.source(name),
                &kind,
                SendDeadline::new(120).expect("budget"),
            )
            .await
            .expect("snapshot");
        let input = PreparedMediaSend {
            options: SendOptions {
                recipient: RecipientSpec::Peer(peer.identity),
                reply_to_message_id: None,
                topic_id: None,
                silent: true,
            },
            caption: " literal caption ".into(),
            media: kind.clone(),
            staged: Arc::clone(&staged),
        };
        let uploaded = Uploaded::from_raw(
            tl::types::InputFile {
                id: 739,
                parts: 1,
                name: staged.upload_name().into(),
                md5_checksum: "offline".into(),
            }
            .into(),
        );
        let request = prepare_media_request(
            uploaded,
            &input,
            &peer,
            &ValidatedTargets { reply_to: None },
            42,
        )
        .expect("request");
        assert_eq!(request.random_id, 42);
        assert_eq!(request.message, " literal caption ");
        assert!(request.silent);
        assert!(!request.clear_draft);
        assert!(!request.allow_paid_floodskip);
        assert!(request.allow_paid_stars.is_none());
        assert!(request.send_as.is_none());
        assert!(request.entities.is_none());
        assert!(request.schedule_date.is_none());
        match (kind, request.media) {
            (SendMediaKind::Image, tl::enums::InputMedia::UploadedPhoto(p)) => {
                assert!(!p.spoiler);
                assert!(p.ttl_seconds.is_none());
            }
            (kind, tl::enums::InputMedia::UploadedDocument(d)) => {
                assert!(!d.force_file);
                assert!(d.thumb.is_none());
                assert!(d.ttl_seconds.is_none());
                assert_eq!(d.mime_type, staged.mime_type());
                assert!(d.attributes.iter().any(|a|matches!(a,tl::enums::DocumentAttribute::Filename(f) if f.file_name==staged.upload_name())));
                match kind {
                    SendMediaKind::Audio{..}=>assert!(d.attributes.iter().any(|a|matches!(a,tl::enums::DocumentAttribute::Audio(a) if !a.voice && a.duration==7 && a.title.as_deref()==Some("title") && a.performer.as_deref()==Some("artist") && a.waveform.is_none()))),
                    SendMediaKind::Voice{..}=>assert!(d.attributes.iter().any(|a|matches!(a,tl::enums::DocumentAttribute::Audio(a) if a.voice && a.duration==8 && a.title.is_none() && a.waveform.is_none()))),
                    SendMediaKind::Video{..}=>assert!(d.attributes.iter().any(|a|matches!(a,tl::enums::DocumentAttribute::Video(v) if !v.round_message && !v.supports_streaming && v.duration==12.5 && v.w==1280 && v.h==720))),
                    _=>panic!("wrong document kind"),
                }
            }
            _ => panic!("kind changed"),
        }
        drop(input);
        drop(staged);
        store
            .close(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            .await
            .expect("cleanup before next kind");
    }
}

#[tokio::test(start_paused = true)]
async fn sending_media_deadline_validation_and_upload_share_one_absolute_budget() {
    use crate::telegram::types::sending::{SendFailure, SendFailureKind};
    let budget = SendDeadline::new(120).expect("budget");
    let start = tokio::time::Instant::now();
    let called = std::cell::Cell::new(false);
    let failure = validate_before_upload(
        budget,
        async {
            tokio::time::sleep(std::time::Duration::from_secs(70)).await;
            Ok(ValidatedTargets { reply_to: None })
        },
        || async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Err::<Uploaded, _>(SendFailure::not_sent(SendFailureKind::UploadFailed, None))
        },
    )
    .await
    .expect_err("total budget, not 120 seconds per stage");
    assert_eq!((tokio::time::Instant::now() - start).as_secs(), 120);
    assert_eq!(failure.kind(), SendFailureKind::Timeout);
    let budget = SendDeadline::new(120).expect("budget");
    let failure = validate_before_upload(
        budget,
        async {
            Err(SendFailure::not_sent(
                SendFailureKind::InvalidReplyTarget,
                None,
            ))
        },
        || async {
            called.set(true);
            Err(SendFailure::not_sent(SendFailureKind::UploadFailed, None))
        },
    )
    .await
    .expect_err("invalid reply");
    assert_eq!(failure.kind(), SendFailureKind::InvalidReplyTarget);
    assert!(!called.get());
}

#[test]
fn sending_media_provider_validation_refusals_are_definitive_not_sent_failures() {
    use crate::telegram::types::sending::{FailureDeliveryState, SendFailureKind};
    for (name, want) in [
        ("MEDIA_INVALID", SendFailureKind::UnsupportedMedia),
        ("MEDIA_FILE_INVALID", SendFailureKind::UnsupportedMedia),
        (
            "PHOTO_INVALID_DIMENSIONS",
            SendFailureKind::UnsupportedMedia,
        ),
        (
            "VIDEO_CONTENT_TYPE_INVALID",
            SendFailureKind::UnsupportedMedia,
        ),
        ("INPUT_FILE_INVALID", SendFailureKind::UnsupportedMedia),
        ("VOICE_MESSAGES_FORBIDDEN", SendFailureKind::SendForbidden),
    ] {
        let error = super::send_transport::classify_final_error(
            grammers_mtsender::InvocationError::Rpc(grammers_mtsender::RpcError {
                code: 400,
                name: name.into(),
                value: None,
                caused_by: None,
            }),
        );
        assert_eq!(error.kind(), want);
        assert_eq!(error.delivery_state(), FailureDeliveryState::NotSent);
        assert!(!error.retryable());
    }
}

#[test]
fn sending_media_upload_errors_keep_typed_wait_hints_without_raw_sources() {
    use crate::telegram::types::sending::{FailureDeliveryState, SendFailureKind};
    let error = super::send_media::upload_failure(std::io::Error::other(
        grammers_mtsender::InvocationError::Rpc(grammers_mtsender::RpcError {
            code: 420,
            name: "FLOOD_WAIT".into(),
            value: Some(27),
            caused_by: None,
        }),
    ));
    assert_eq!(error.kind(), SendFailureKind::RateLimited);
    assert_eq!(error.retry_after_seconds(), Some(27));
    assert_eq!(error.delivery_state(), FailureDeliveryState::NotSent);
    let error =
        super::send_media::upload_failure(std::io::Error::other("secret upload diagnostic"));
    assert_eq!(error.kind(), SendFailureKind::UploadFailed);
    assert!(!format!("{error:?} {error}").contains("secret"));
}
