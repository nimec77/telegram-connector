use super::probe::probe;
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{SendFailureKind, SendMediaKind};
use std::io::{Seek as _, Write as _};

fn inspect(
    bytes: &[u8],
    kind: &SendMediaKind,
) -> Result<super::probe::ProbedMedia, crate::telegram::types::sending::SendFailure> {
    let mut file = tempfile::tempfile().expect("fixture");
    file.write_all(bytes).expect("bytes");
    file.rewind().expect("rewind");
    probe(
        &mut file,
        kind,
        bytes.len() as u64,
        SendDeadline::new(30).expect("budget"),
    )
}
#[test]
fn sending_probe_accepts_only_bounded_plausible_headers_of_the_requested_kind() {
    let jpeg = crate::test_helpers::create_test_jpeg(16, 16);
    assert_eq!(
        inspect(&jpeg, &SendMediaKind::Image)
            .expect("JPEG")
            .mime_type,
        "image/jpeg"
    );
    assert_eq!(
        inspect(
            b"\xff\xfb\x90\x00abcdefgh",
            &SendMediaKind::Audio {
                duration_seconds: 1,
                title: None,
                performer: None
            }
        )
        .expect("MP3")
        .mime_type,
        "audio/mpeg"
    );
    assert_eq!(
        inspect(b"not media", &SendMediaKind::Image)
            .expect_err("mismatch")
            .kind(),
        SendFailureKind::UnsupportedMedia
    );
    assert!(
        inspect(
            &jpeg,
            &SendMediaKind::Video {
                duration_seconds: 1.0,
                width: 1,
                height: 1
            }
        )
        .is_err()
    );
    let huge_id3 = b"ID3\x04\0\0\x7f\x7f\x7f\x7f";
    assert!(
        inspect(
            huge_id3,
            &SendMediaKind::Audio {
                duration_seconds: 1,
                title: None,
                performer: None
            }
        )
        .is_err()
    );
}

#[test]
fn sending_probe_png_aac_m4a_video_and_opus_have_controlled_types() {
    let audio = SendMediaKind::Audio {
        duration_seconds: 1,
        title: None,
        performer: None,
    };
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(16, 16)
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("PNG fixture");
    assert_eq!(
        inspect(png.get_ref(), &SendMediaKind::Image)
            .expect("PNG")
            .mime_type,
        "image/png"
    );
    assert_eq!(
        inspect(b"\xff\xf1\x50\x80\x01\x7f\xfc", &audio)
            .expect("AAC header")
            .mime_type,
        "audio/aac"
    );
    let m4a = b"\0\0\0\x10ftypM4A \0\0\0\0";
    assert_eq!(
        inspect(m4a, &audio).expect("M4A header").mime_type,
        "audio/mp4"
    );
    let video = SendMediaKind::Video {
        duration_seconds: 1.0,
        width: 16,
        height: 16,
    };
    assert!(inspect(m4a, &video).is_err());
    assert_eq!(
        inspect(b"\0\0\0\x10ftypmp42\0\0\0\0", &video)
            .expect("MP4 header")
            .mime_type,
        "video/mp4"
    );
    let mut ogg = vec![0; 27];
    ogg[..4].copy_from_slice(b"OggS");
    ogg[5] = 2;
    ogg[26] = 1;
    ogg.push(19);
    ogg.extend_from_slice(b"OpusHead\x01\x01\0\0\x80\xbb\0\0\0\0\0");
    assert_eq!(
        inspect(
            &ogg,
            &SendMediaKind::Voice {
                duration_seconds: 1
            }
        )
        .expect("Opus ID header")
        .mime_type,
        "audio/ogg"
    );
    ogg.truncate(38);
    assert!(
        inspect(
            &ogg,
            &SendMediaKind::Voice {
                duration_seconds: 1
            }
        )
        .is_err(),
        "truncated Opus ID metadata"
    );
}

fn audio() -> SendMediaKind {
    SendMediaKind::Audio {
        duration_seconds: 1,
        title: None,
        performer: None,
    }
}

#[test]
fn sending_probe_large_metadata_jpeg_decodes_and_is_accepted() {
    let bytes = super::media_fixtures::metadata_jpeg();
    let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
        .expect("real JPEG remains decodable after APP/COM insertion");
    assert_eq!((decoded.width(), decoded.height()), (16, 16));
    assert_eq!(
        inspect(&bytes, &SendMediaKind::Image)
            .expect("SOF beyond 64 KiB")
            .mime_type,
        "image/jpeg"
    );
}

#[test]
fn sending_probe_large_id3_before_real_mpeg_is_accepted() {
    let bytes = super::media_fixtures::tagged_mp3(4, 0, &vec![0; 131_072]);
    assert_eq!(
        inspect(&bytes, &audio())
            .expect("MPEG beyond large ID3")
            .mime_type,
        "audio/mpeg"
    );
}

#[test]
fn sending_probe_restores_original_position_on_success_and_failure() {
    for (bytes, kind, accepted) in [
        (
            super::media_fixtures::metadata_jpeg(),
            SendMediaKind::Image,
            true,
        ),
        (
            super::media_fixtures::tagged_mp3(4, 0x10, &[0; 16]),
            audio(),
            true,
        ),
        (b"not a JPEG".to_vec(), SendMediaKind::Image, false),
        (b"ID3\x04\0\0\x7f\x7f\x7f\x7f".to_vec(), audio(), false),
    ] {
        let mut file = tempfile::tempfile().expect("fixture");
        file.write_all(&bytes).expect("bytes");
        for original in [0, 5, bytes.len() as u64, bytes.len() as u64 + 17] {
            file.seek(std::io::SeekFrom::Start(original))
                .expect("caller position");
            let result = probe(
                &mut file,
                &kind,
                bytes.len() as u64,
                SendDeadline::new(30).expect("budget"),
            );
            assert_eq!(result.is_ok(), accepted);
            assert_eq!(file.stream_position().expect("position"), original);
        }
    }
}

#[test]
fn sending_probe_id3_version_flags_and_footer_are_validated() {
    for version in [2, 3, 4] {
        assert!(
            inspect(
                &super::media_fixtures::tagged_mp3(version, 0, &[0; 16]),
                &audio()
            )
            .is_ok()
        );
    }

    for (version, flags) in [(1, 0), (5, 0), (255, 0), (2, 0x20), (3, 0x10), (4, 1)] {
        assert!(
            inspect(
                &super::media_fixtures::tagged_mp3(version, flags, &[0; 16]),
                &audio()
            )
            .is_err(),
            "invalid version={version} flags={flags}"
        );
    }
}

// Small header-only fixtures exercise container bounds, never codec validity.
fn header_jpeg(metadata_markers: usize, fill: usize) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8];
    for _ in 0..metadata_markers {
        bytes.extend_from_slice(&[0xff, 0xfe, 0, 2]);
    }
    bytes.extend(std::iter::repeat_n(0xff, fill));
    bytes.extend_from_slice(&[0xff, 0xc0, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
    bytes.extend_from_slice(&[0xff, 0xda, 0, 8, 1, 1, 0, 0, 63, 0, 1, 0xff, 0xd9]);
    bytes
}

#[test]
fn sending_probe_jpeg_marker_limit_includes_soi_sof_and_sos() {
    assert!(
        inspect(&header_jpeg(4093, 0), &SendMediaKind::Image).is_ok(),
        "4096 markers"
    );
    assert!(
        inspect(&header_jpeg(4094, 0), &SendMediaKind::Image).is_err(),
        "4097 markers"
    );
}

#[test]
fn sending_probe_rejects_malformed_headers_even_after_dimensions() {
    let valid = header_jpeg(0, 0);
    let cases = [
        vec![0xff, 0xd8, 0xff, 0xfe, 0, 1],
        vec![0xff, 0xd8, 0xff, 0xe1, 0xff, 0xff, 0],
        vec![0xff, 0xd8, 0xff, 0xd9],
        vec![0xff, 0xd8, 0xff, 0, 0, 2],
        vec![0xff, 0xd8, 0xff, 0xd8],
        {
            let mut b = valid.clone();
            b[8] = 0;
            b
        }, // zero height
        {
            let mut b = valid.clone();
            b[15..19].copy_from_slice(&[0xff, 0xfe, 0xff, 0xff]);
            b
        },
        valid[..15].to_vec(), // SOF alone has no scan header
    ];
    for (i, bytes) in cases.iter().enumerate() {
        assert!(
            inspect(bytes, &SendMediaKind::Image).is_err(),
            "malformed case {i}"
        );
    }
}

#[test]
fn sending_probe_id3_v24_footer_is_outside_body_size() {
    assert!(
        inspect(
            &super::media_fixtures::tagged_mp3(4, 0x10, &[0; 16]),
            &audio()
        )
        .is_ok(),
        "v2.4 footer excluded from body size"
    );
}

#[test]
fn sending_probe_jpeg_actual_read_budget_is_separate_from_seek_distance() {
    // SOI=2, SOF marker/length/dimensions=10, SOS marker/length/count=5.
    // Component tables and metadata payloads are skipped, not inspected.
    assert!(
        inspect(&header_jpeg(0, 65_519), &SendMediaKind::Image).is_ok(),
        "exactly 65536 inspected bytes"
    );
    assert!(
        inspect(&header_jpeg(0, 65_520), &SendMediaKind::Image).is_err(),
        "65537 inspected bytes"
    );
}

#[test]
fn sending_probe_snapshot_boundary_cannot_be_crossed_by_reads_or_seeks() {
    for (bytes, kind) in [
        (header_jpeg(0, 0), SendMediaKind::Image),
        (super::media_fixtures::tagged_mp3(4, 0, &[0; 16]), audio()),
    ] {
        let mut file = tempfile::tempfile().expect("file");
        file.write_all(&bytes).expect("write");
        file.seek(std::io::SeekFrom::Start(5)).expect("position");
        let result = probe(&mut file, &kind, 10, SendDeadline::new(30).expect("budget"));
        assert_eq!(
            result
                .expect_err("bytes after snapshot are inaccessible")
                .kind(),
            SendFailureKind::UnsupportedMedia
        );
        assert_eq!(file.stream_position().expect("restored"), 5);
    }
}

#[tokio::test(start_paused = true)]
async fn sending_probe_original_expired_deadline_is_not_restarted_and_position_is_restored() {
    let bytes = super::media_fixtures::metadata_jpeg();
    let mut file = tempfile::tempfile().expect("file");
    file.write_all(&bytes).expect("bytes");
    file.seek(std::io::SeekFrom::Start(7)).expect("position");
    let deadline = SendDeadline::new(1).expect("budget");
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    let result = probe(
        &mut file,
        &SendMediaKind::Image,
        bytes.len() as u64,
        deadline,
    );
    assert_eq!(
        result.expect_err("original expiry").kind(),
        SendFailureKind::Timeout
    );
    assert_eq!(file.stream_position().expect("position"), 7);
}

#[test]
fn sending_probe_id3_rejects_malformed_declared_metadata() {
    let valid = super::media_fixtures::tagged_mp3(4, 0, &[0; 16]);
    for (name, bytes) in [
        ("revision", {
            let mut b = valid.clone();
            b[4] = 255;
            b
        }),
        ("synchsafe", {
            let mut b = valid.clone();
            b[6] = 128;
            b
        }),
        ("oversized", b"ID3\x04\0\0\x7f\x7f\x7f\x7f".to_vec()),
        ("short header", b"ID3\x04\0".to_vec()),
        (
            "bad extended v3",
            super::media_fixtures::tagged_mp3(3, 0x40, &[0; 16]),
        ),
        (
            "bad extended v4",
            super::media_fixtures::tagged_mp3(4, 0x40, &[0; 16]),
        ),
        ("bad footer", {
            let mut b = super::media_fixtures::tagged_mp3(4, 0x10, &[0; 16]);
            b[26] = 0;
            b
        }),
        ("mismatched footer", {
            let mut b = super::media_fixtures::tagged_mp3(4, 0x10, &[0; 16]);
            b[35] = 15;
            b
        }),
    ] {
        assert!(inspect(&bytes, &audio()).is_err(), "{name}");
    }
}

#[test]
fn sending_probe_valid_version_specific_id3_extended_headers_and_flags() {
    for (version, flags, body) in [
        (2, 0xc0, vec![0; 16]),
        (3, 0xa0, vec![0; 16]),
        (3, 0x40, vec![0, 0, 0, 6, 0, 0, 0, 0, 0, 0]),
        (4, 0xc0, vec![0, 0, 0, 6, 1, 0]),
        (4, 0x50, vec![0, 0, 0, 6, 1, 0]),
    ] {
        let bytes = super::media_fixtures::tagged_mp3(version, flags, &body);
        assert_eq!(
            inspect(&bytes, &audio())
                .expect("version-specific length accounting")
                .mime_type,
            "audio/mpeg"
        );
    }
}

#[test]
fn sending_probe_id3_extended_headers_and_footer_cannot_cross_tag_or_snapshot() {
    for (version, body) in [
        (3, vec![0, 0, 0, 6]), // length excludes its four-byte size field
        (3, vec![0xff, 0xff, 0xff, 0xff]),
        (4, vec![0, 0, 0, 6]), // inclusive length still exceeds body
        (4, vec![0x80, 0, 0, 6, 1, 0]),
    ] {
        assert!(
            inspect(
                &super::media_fixtures::tagged_mp3(version, 0x40, &body),
                &audio()
            )
            .is_err()
        );
    }
    let mut footer = super::media_fixtures::tagged_mp3(4, 0x10, &[0; 16]);
    footer.truncate(35); // nine of the required ten footer bytes
    assert!(inspect(&footer, &audio()).is_err());
    let mut no_mpeg = super::media_fixtures::tagged_mp3(4, 0, &[0; 16]);
    no_mpeg[26..30].fill(0);
    assert!(
        inspect(&no_mpeg, &audio()).is_err(),
        "ID3 alone does not establish MP3"
    );
}
