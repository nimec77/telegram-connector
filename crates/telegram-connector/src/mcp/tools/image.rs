//! Bounded JPEG and base64 previews for the existing stdio MCP image contract.

use crate::error::Error;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use telegram_core::operations::preview::shape_jpeg;
pub use telegram_core::operations::preview::{
    MAX_DECODED_BYTES, MAX_DECODED_DIMENSION, MAX_DECODED_PIXELS,
};

/// Maximum allowed base64 payload length, in characters (~1.5 MB).
pub const MAX_BASE64_LEN: usize = 1_572_864;

pub(crate) fn base64_len(bytes: usize) -> Option<usize> {
    bytes.checked_add(2)?.checked_div(3)?.checked_mul(4)
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
///
/// Never upscales. Already-fitting JPEG data is preserved byte for byte; other
/// inputs become JPEG quality 80 and may shrink further to [`MAX_BASE64_LEN`].
///
/// # Errors
/// Returns [`Error::ImageLimitExceeded`] for oversized source images,
/// [`Error::DownloadFailed`] on decode/encode failure or
/// [`Error::PayloadCapExceeded`] when bounded shrink attempts cannot fit the cap.
/// No network download or account charge occurs here.
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
    let result = shape_jpeg(bytes, max_dimension, max_base64_len, base64_len)?;
    Ok(ProcessedImage {
        width: result.width,
        height: result.height,
        encoded_size_bytes: result.jpeg.len(),
        base64_jpeg: BASE64.encode(&result.jpeg),
    })
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
        assert_eq!(&decoded[..2], [0xff, 0xd8], "output must be JPEG");
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

#[cfg(test)]
mod allocation_tests {
    use super::*;
    use image::DynamicImage;
    use std::io::Cursor;

    #[test]
    fn task8_base64_length_handles_quantum_boundaries_and_overflow() {
        for (bytes, expected) in [
            (0, Some(0)),
            (1, Some(4)),
            (2, Some(4)),
            (3, Some(4)),
            (4, Some(8)),
            (5, Some(8)),
            (6, Some(8)),
            (usize::MAX, None),
            (usize::MAX - 2, None),
        ] {
            assert_eq!(base64_len(bytes), expected);
        }
    }

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(1, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .expect("PNG fixture");
        let mut bytes = encoded.into_inner();
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        let mut crc = u32::MAX;
        for byte in &bytes[12..29] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
        bytes
    }

    #[test]
    fn task8_rejects_excessive_image_dimensions_before_decoding_pixels() {
        for (width, height) in [(8193, 1), (1, 8193), (4097, 4096), (4097, 2048)] {
            let error = process_image(&png_header(width, height), 1280).expect_err("reject header");
            let error = crate::mcp::tools::types::contract::ToolError::from(error);
            assert_eq!(
                serde_json::to_value(error).expect("error")["code"],
                "media_too_large",
                "{width}x{height}"
            );
        }
    }
}
