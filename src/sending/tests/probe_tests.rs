use super::probe::probe;
use crate::telegram::types::sending::{SendFailureKind, SendMediaKind};
use std::io::{Seek as _, Write as _};

fn inspect(
    bytes: &[u8],
    kind: &SendMediaKind,
) -> Result<super::probe::ProbedMedia, crate::telegram::types::sending::SendFailure> {
    let mut file = tempfile::tempfile().expect("fixture");
    file.write_all(bytes).expect("bytes");
    file.rewind().expect("rewind");
    probe(&mut file, kind)
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
