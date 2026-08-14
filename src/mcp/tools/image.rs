//! Pure image processing pipeline for get_message_media.
//!
//! Decode → downscale (longest side <= max_dimension) → JPEG q80 → base64,
//! shrinking iteratively until the base64 payload fits the cap. No I/O.

use crate::error::Error;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::DynamicImage;
use image::codecs::jpeg::JpegEncoder;
use std::io::Cursor;

/// Maximum allowed base64 payload length, in characters (~1.5 MB).
pub const MAX_BASE64_LEN: usize = 1_572_864;
/// JPEG re-encode quality.
const JPEG_QUALITY: u8 = 80;
/// Cap-fitting iterations before giving up (each shrinks >= 10%, so this is
/// a >40% total reduction — far beyond what any real photo needs).
const MAX_CAP_ITERATIONS: usize = 5;

/// JPEG SOI magic — cheap already-JPEG sniff for the passthrough branch.
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8])
}

/// A processed image ready to be returned as an MCP image content block.
#[derive(Debug, Clone)]
pub struct ProcessedImage {
    /// Base64-encoded JPEG (standard alphabet, padded).
    pub base64_jpeg: String,
    pub width: u32,
    pub height: u32,
    /// Encoded JPEG size in bytes, before base64 expansion.
    pub encoded_size_bytes: usize,
}

/// Downscale and re-encode an image for return over MCP.
pub fn process_image(bytes: &[u8], max_dimension: u32) -> Result<ProcessedImage, Error> {
    process_image_with_cap(bytes, max_dimension, MAX_BASE64_LEN)
}

/// Same as [`process_image`] with an explicit payload cap (separated for tests:
/// producing >1.5 MB of JPEG in a unit test would be slow).
pub(crate) fn process_image_with_cap(
    bytes: &[u8],
    max_dimension: u32,
    max_base64_len: usize,
) -> Result<ProcessedImage, Error> {
    let decoded = image::load_from_memory(bytes)
        .map_err(|e| Error::DownloadFailed(format!("failed to decode image: {e}")))?;

    // Already-JPEG sources that need no downscale pass through untouched —
    // re-encoding at identical dimensions degrades quality and can grow the
    // payload (work-order D4 measured +29%).
    let passthrough_base64_len = bytes.len().div_ceil(3) * 4;
    if is_jpeg(bytes)
        && decoded.width().max(decoded.height()) <= max_dimension
        && passthrough_base64_len <= max_base64_len
    {
        return Ok(ProcessedImage {
            width: decoded.width(),
            height: decoded.height(),
            encoded_size_bytes: bytes.len(),
            base64_jpeg: BASE64.encode(bytes),
        });
    }

    let mut target = max_dimension;
    for _ in 0..MAX_CAP_ITERATIONS {
        let resized = downscale(&decoded, target);
        let jpeg = encode_jpeg(&resized)?;
        let encoded = BASE64.encode(&jpeg);
        if encoded.len() <= max_base64_len {
            return Ok(ProcessedImage {
                width: resized.width(),
                height: resized.height(),
                encoded_size_bytes: jpeg.len(),
                base64_jpeg: encoded,
            });
        }
        // Area scales with the square of the side, so sqrt of the byte ratio
        // estimates the needed shrink; never shrink less than 10% per round
        // so the loop provably converges.
        let ratio = (max_base64_len as f64 / encoded.len() as f64)
            .sqrt()
            .min(0.9);
        target = (f64::from(target) * ratio).floor().max(1.0) as u32;
    }

    Err(Error::PayloadCapExceeded {
        limit: max_base64_len,
    })
}

fn downscale(img: &DynamicImage, max_dimension: u32) -> DynamicImage {
    if img.width().max(img.height()) <= max_dimension {
        img.clone()
    } else {
        // resize() preserves aspect ratio within the bounding box. Lanczos3
        // keeps text and chart lines legible — the primary use case.
        img.resize(
            max_dimension,
            max_dimension,
            image::imageops::FilterType::Lanczos3,
        )
    }
}

fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>, Error> {
    // JPEG has no alpha; convert unconditionally so RGBA sources cannot fail.
    let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
    let mut buf = Cursor::new(Vec::new());
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY))
        .map_err(|e| Error::DownloadFailed(format!("failed to encode JPEG: {e}")))?;
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::create_test_jpeg;

    #[test]
    fn downscales_to_max_dimension_preserving_aspect() {
        let jpeg = create_test_jpeg(200, 100);
        let processed = process_image(&jpeg, 100).unwrap();
        assert_eq!(processed.width, 100);
        assert_eq!(processed.height, 50);
    }

    #[test]
    fn never_upscales_small_images() {
        let jpeg = create_test_jpeg(50, 25);
        let processed = process_image(&jpeg, 1280).unwrap();
        assert_eq!(processed.width, 50);
        assert_eq!(processed.height, 25);
    }

    #[test]
    fn output_is_valid_base64_jpeg() {
        use base64::Engine as _;
        let jpeg = create_test_jpeg(64, 64);
        let processed = process_image(&jpeg, 64).unwrap();
        let decoded_bytes = base64::engine::general_purpose::STANDARD
            .decode(&processed.base64_jpeg)
            .expect("output must be valid base64");
        assert_eq!(decoded_bytes.len(), processed.encoded_size_bytes);
        let img = image::load_from_memory(&decoded_bytes).expect("output must be a decodable JPEG");
        assert_eq!(img.width(), 64);
    }

    #[test]
    fn shrinks_until_payload_cap_is_met() {
        // 512px noisy JPEG is far over a 10 KB cap; the loop must shrink it under.
        let jpeg = create_test_jpeg(512, 512);
        let processed = process_image_with_cap(&jpeg, 512, 10_000).unwrap();
        assert!(processed.base64_jpeg.len() <= 10_000);
        assert!(processed.width < 512);
    }

    #[test]
    fn invalid_bytes_return_download_failed() {
        let result = process_image(b"not an image at all", 1280);
        match result {
            Err(crate::error::Error::DownloadFailed(msg)) => {
                assert!(msg.contains("decode"));
            }
            other => panic!("expected DownloadFailed, got {other:?}"),
        }
    }

    #[test]
    fn jpeg_within_max_dimension_passes_through_byte_identical() {
        let jpeg = create_test_jpeg(100, 50);
        let processed = process_image(&jpeg, 1280).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&processed.base64_jpeg)
            .unwrap();
        assert_eq!(decoded, jpeg, "no re-encode may occur");
        assert_eq!(processed.encoded_size_bytes, jpeg.len());
        assert_eq!((processed.width, processed.height), (100, 50));
    }

    #[test]
    fn non_jpeg_source_is_still_reencoded_to_jpeg() {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            80,
            40,
            image::Rgb([10, 200, 30]),
        ));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let processed = process_image(png.get_ref(), 1280).unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&processed.base64_jpeg)
            .unwrap();
        assert_eq!(&decoded[..2], [0xFF, 0xD8], "output must be JPEG");
    }

    #[test]
    fn cap_exhaustion_returns_payload_cap_exceeded_not_download_failed() {
        // A 100-byte cap is unreachable: even a heavily downscaled JPEG carries
        // several hundred bytes of headers, so the shrink loop provably exhausts.
        // The passthrough branch is skipped too (its base64 length exceeds 100).
        let jpeg = create_test_jpeg(64, 64);
        let err = process_image_with_cap(&jpeg, 64, 100).expect_err("cap is unreachable");
        assert!(
            matches!(err, Error::PayloadCapExceeded { limit: 100 }),
            "cap exhaustion must be its own variant, got: {err:?}"
        );
    }
}
