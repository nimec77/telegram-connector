//! Image decoder and bounded JPEG shaping shared by stdio and hosted media.

use crate::error::Error;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageDecoder as _, ImageReader, Limits};
use std::borrow::Cow;
use std::io::Cursor;

/// Maximum source side length, checked before decoding pixels.
pub const MAX_DECODED_DIMENSION: u32 = 8192;
/// Maximum decoded source area (8 megapixels, including 4K frames).
pub const MAX_DECODED_PIXELS: u64 = 8_388_608;
/// Decoder allocation allowance, including its output image, excluding allocator overhead.
pub const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;

fn decode_error(error: image::ImageError) -> Error {
    match error {
        image::ImageError::Limits(_) => Error::ImageLimitExceeded,
        other => Error::DownloadFailed(format!("failed to decode image: {other}")),
    }
}

fn decode_bounded(bytes: &[u8]) -> Result<DynamicImage, Error> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_DIMENSION);
    limits.max_image_height = Some(MAX_DECODED_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::DownloadFailed("failed to detect image format".into()))?;
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_DECODED_PIXELS
        || decoder.total_bytes() > MAX_DECODED_BYTES
    {
        return Err(Error::ImageLimitExceeded);
    }
    limits
        .reserve(decoder.total_bytes())
        .map_err(decode_error)?;
    decoder.set_limits(limits).map_err(decode_error)?;
    DynamicImage::from_decoder(decoder).map_err(decode_error)
}

/// JPEG re-encode quality.
const JPEG_QUALITY: u8 = 80;
/// Cap-fitting iterations before giving up (each shrinks >= 10%, so this is
/// a >40% total reduction — far beyond what any real photo needs).
const MAX_CAP_ITERATIONS: usize = 5;

/// JPEG SOI magic — cheap already-JPEG sniff for the passthrough branch.
fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xff, 0xd8])
}

/// Bounded preview bytes and their actual pixel dimensions.
pub struct EncodedPreview<'a> {
    /// A fitting source JPEG is borrowed unchanged.
    pub jpeg: Cow<'a, [u8]>,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

/// Decode and shape JPEG bytes using a transport's encoded-size rule.
///
/// # Errors
/// Returns a typed image limit, decoder/encoder error or exhausted payload cap.
pub fn shape_jpeg(
    bytes: &[u8],
    max_dimension: u32,
    cap: usize,
    measured_size: fn(usize) -> Option<usize>,
) -> Result<EncodedPreview<'_>, Error> {
    let decoded = decode_bounded(bytes)?;
    let size = |len| measured_size(len).ok_or(Error::PayloadCapExceeded { limit: cap });
    if is_jpeg(bytes)
        && decoded.width().max(decoded.height()) <= max_dimension
        && size(bytes.len())? <= cap
    {
        return Ok(EncodedPreview {
            width: decoded.width(),
            height: decoded.height(),
            jpeg: Cow::Borrowed(bytes),
        });
    }

    let mut target = max_dimension;
    for _ in 0..MAX_CAP_ITERATIONS {
        let resized = downscale(&decoded, target);
        let jpeg = encode_jpeg(&resized)?;
        let encoded_len = size(jpeg.len())?;
        if encoded_len <= cap {
            return Ok(EncodedPreview {
                width: resized.width(),
                height: resized.height(),
                jpeg: Cow::Owned(jpeg),
            });
        }
        // Scale by the square root of the byte ratio to reflect pixel area.
        let ratio = (cap as f64 / encoded_len as f64).sqrt().min(0.9);
        target = (f64::from(target) * ratio).floor().max(1.0) as u32;
    }

    Err(Error::PayloadCapExceeded { limit: cap })
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
