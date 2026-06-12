# get_message_media Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** New MCP tool #10 `get_message_media` that returns a message's photo (or a video's server-side thumbnail) as an MCP image content block plus a JSON metadata text block.

**Architecture:** A new `TelegramClientTrait::download_message_media` method downloads the best-fitting server-side size variant in memory via grammers `iter_download`; a pure image pipeline (`src/mcp/tools/image.rs`) downscales/re-encodes to JPEG q80 under a 1.5 MB base64 cap; the tool handler in `server.rs` returns `Result<CallToolResult, String>` (the one sanctioned deviation from the all-tools-return-`String` convention — rmcp's real constraint is `IntoCallToolResult`). Spec: `docs/superpowers/specs/2026-06-12-get-message-media-design.md`.

**Tech Stack:** Rust nightly (2024 edition), rmcp 1.7, grammers (git master), `image` 0.25 (jpeg feature only), `base64` 0.22, mockall.

**Branch:** `feat/get-message-media` (already created; spec committed).

**Verification gate after every task:** `cargo fmt --all && cargo clippy -- -D warnings && cargo test` (config tests are serial: `cargo test config -- --test-threads=1` if running them alone).

---

### Task 1: Add dependencies

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1.1: Add `image` and `base64` to `[dependencies]`**

In `Cargo.toml`, after the `# Security` block (`secrecy = ...`), add:

```toml
# Media processing
image = { version = "0.25", default-features = false, features = ["jpeg"] }
base64 = "0.22"
```

- [ ] **Step 1.2: Verify it builds**

Run: `cargo build`
Expected: compiles cleanly (downloads new crates on first run).

- [ ] **Step 1.3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add image and base64 dependencies for get_message_media"
```

---

### Task 2: New error variants

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 2.1: Write the failing tests**

Add to `mod tests` in `src/error.rs` (follow the existing Display-test convention):

```rust
#[test]
fn test_media_too_large_error_display() {
    let error = Error::MediaTooLarge {
        size_bytes: 25_000_000,
        max_bytes: 20_971_520,
    };
    assert_eq!(
        error.to_string(),
        "media too large: 25000000 bytes exceeds limit of 20971520 bytes"
    );
}

#[test]
fn test_no_visual_media_error_display() {
    let error = Error::NoVisualMedia {
        media_type: "poll".to_string(),
    };
    assert_eq!(
        error.to_string(),
        "message has no visual media (media type: poll)"
    );
}

#[test]
fn test_download_failed_error_display() {
    let error = Error::DownloadFailed("connection reset".to_string());
    assert_eq!(error.to_string(), "media download failed: connection reset");
}
```

- [ ] **Step 2.2: Run tests to verify they fail**

Run: `cargo test error`
Expected: FAIL — `no variant or associated item named 'MediaTooLarge'` (compile error).

- [ ] **Step 2.3: Add the variants**

In the `Error` enum in `src/error.rs`, after the `Timeout` variant:

```rust
#[error("media too large: {size_bytes} bytes exceeds limit of {max_bytes} bytes")]
MediaTooLarge { size_bytes: u64, max_bytes: u64 },

#[error("message has no visual media (media type: {media_type})")]
NoVisualMedia { media_type: String },

#[error("media download failed: {0}")]
DownloadFailed(String),
```

- [ ] **Step 2.4: Run tests to verify they pass**

Run: `cargo test error`
Expected: PASS (all error tests, including the 3 new ones).

- [ ] **Step 2.5: Commit**

```bash
cargo fmt --all
git add src/error.rs
git commit -m "feat: add MediaTooLarge, NoVisualMedia, DownloadFailed error variants"
```

---

### Task 3: Config fields

**Files:**
- Modify: `src/config.rs`
- Test: `src/config/tests.rs`

Three new fields, all with serde defaults so existing configs keep working:
- `[rate_limiting] media_download_cost` (u32, default 5)
- `[telegram.timeouts] download_secs` (u64, default 120, must be > 0)
- `[observability] max_buffered_payload_bytes` (usize, default 262_144)

- [ ] **Step 3.1: Write the failing tests**

Add to `src/config/tests.rs` (config tests use `toml::from_str` directly; they run serial — that constraint is about env-var tests, but keep the convention):

```rust
#[test]
fn test_media_download_cost_default() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.rate_limiting.media_download_cost, 5);
}

#[test]
fn test_media_download_cost_from_toml() {
    let toml_str = "[telegram]\napi_id = 12345\n[rate_limiting]\nmedia_download_cost = 9\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.rate_limiting.media_download_cost, 9);
}

#[test]
fn test_download_secs_default() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.telegram.timeouts.download_secs, 120);
}

#[test]
fn test_download_secs_from_toml() {
    let toml_str = "[telegram]\napi_id = 12345\n[telegram.timeouts]\ndownload_secs = 60\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.telegram.timeouts.download_secs, 60);
}

#[test]
fn test_download_secs_zero_fails_validation() {
    let toml_str = "[telegram]\napi_id = 12345\n[telegram.timeouts]\ndownload_secs = 0\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    let result = config.telegram.timeouts.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("download_secs"));
}

#[test]
fn test_max_buffered_payload_bytes_default() {
    let config: Config = toml::from_str("[telegram]\napi_id = 12345\n").unwrap();
    assert_eq!(config.observability.max_buffered_payload_bytes, 262_144);
}

#[test]
fn test_max_buffered_payload_bytes_from_toml() {
    let toml_str = "[telegram]\napi_id = 12345\n[observability]\nmax_buffered_payload_bytes = 1024\n";
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.observability.max_buffered_payload_bytes, 1024);
}
```

Note: if `TimeoutConfig::validate()` is not `pub` or named differently, check `src/config.rs:177` — it exists and bails on zero `resolve_secs`/`history_secs`; mirror that exact pattern.

- [ ] **Step 3.2: Run tests to verify they fail**

Run: `cargo test config -- --test-threads=1`
Expected: FAIL — compile errors for the unknown fields.

- [ ] **Step 3.3: Implement the config fields**

In `src/config.rs`:

1. Default fns (next to the existing `default_*` fns, e.g. after `default_refill_rate`):

```rust
fn default_media_download_cost() -> u32 {
    5
}

fn default_download_secs() -> u64 {
    120
}

fn default_max_buffered_payload_bytes() -> usize {
    262_144
}
```

2. `RateLimitConfig` — add field:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_refill_rate")]
    pub refill_rate: f64,
    /// Tokens charged per get_message_media call (searches cost 1).
    #[serde(default = "default_media_download_cost")]
    pub media_download_cost: u32,
}
```

3. `default_rate_limit_config()` — add `media_download_cost: default_media_download_cost(),`.

4. `TimeoutConfig` — add field (after `history_secs`):

```rust
/// Wall-clock budget for a full in-memory media download.
#[serde(default = "default_download_secs")]
pub download_secs: u64,
```

5. `default_timeout_config()` — add `download_secs: default_download_secs(),`.

6. `TimeoutConfig::validate()` — add:

```rust
if self.download_secs == 0 {
    anyhow::bail!("telegram.timeouts.download_secs must be > 0");
}
```

7. `ObservabilityConfig` — add field:

```rust
/// Responses larger than this many bytes are recorded in the ring buffer
/// with a stub payload instead of the full body (get_message_media responses
/// are ~1.5 MB of base64; replaying them via get_last_responses is useless).
#[serde(default = "default_max_buffered_payload_bytes")]
pub max_buffered_payload_bytes: usize,
```

8. `impl Default for ObservabilityConfig` — add `max_buffered_payload_bytes: default_max_buffered_payload_bytes(),`.

- [ ] **Step 3.4: Run tests to verify they pass**

Run: `cargo test config -- --test-threads=1`
Expected: PASS. Also run `cargo test` to confirm nothing else broke (struct literals of these configs in other tests may need the new fields — fix any by adding the default values).

- [ ] **Step 3.5: Commit**

```bash
cargo fmt --all
git add src/config.rs src/config/tests.rs
git commit -m "feat: add media_download_cost, download_secs, max_buffered_payload_bytes config"
```

---

### Task 4: Domain types — MediaDownload and SizeCandidate

**Files:**
- Modify: `src/telegram/types/media.rs`
- Modify: `src/telegram/types.rs` (re-exports)

- [ ] **Step 4.1: Write the failing test**

Add to `mod tests` in `src/telegram/types/media.rs`:

```rust
#[test]
fn media_download_construction() {
    let download = MediaDownload {
        bytes: vec![0xff, 0xd8],
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: Some("a chart".to_string()),
        width: Some(1280),
        height: Some(720),
        source_size_bytes: 2,
    };
    assert_eq!(download.media_type, MediaType::Photo);
    assert!(!download.is_thumbnail);
}

#[test]
fn size_candidate_construction() {
    let candidate = SizeCandidate {
        width: 800,
        height: 600,
        size_bytes: 50_000,
        photo_type: "x".to_string(),
    };
    assert_eq!(candidate.width.max(candidate.height), 800);
}
```

- [ ] **Step 4.2: Run tests to verify they fail**

Run: `cargo test media`
Expected: FAIL — `cannot find struct ... MediaDownload` (compile error).

- [ ] **Step 4.3: Add the types**

In `src/telegram/types/media.rs`, after the `MediaFilter` enum:

```rust
/// Raw media bytes downloaded from Telegram plus source metadata.
///
/// Produced by `TelegramClientTrait::download_message_media`; consumed by the
/// MCP-layer image pipeline. Carries no grammers types so it can flow through
/// the mockable trait boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaDownload {
    /// Raw downloaded image bytes (JPEG as served by Telegram).
    pub bytes: Vec<u8>,
    /// What the source message's media was (Photo, Video, Animation, VideoNote).
    pub media_type: MediaType,
    /// True when `bytes` is a video-like media's thumbnail, not the media itself.
    pub is_thumbnail: bool,
    /// Message caption (`msg.text()` on media messages), None when empty.
    pub caption: Option<String>,
    /// Pixel width of the downloaded size variant, if Telegram reported it.
    pub width: Option<u32>,
    /// Pixel height of the downloaded size variant, if Telegram reported it.
    pub height: Option<u32>,
    /// Byte size of the downloaded size variant.
    pub source_size_bytes: u64,
}

/// A downloadable size variant of a photo or thumbnail, decoupled from
/// grammers `PhotoSize` so size selection is a pure, testable function.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeCandidate {
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
    /// Telegram thumbnail type tag (e.g. "m", "x", "y") used to map the
    /// selection back to the grammers PhotoSize to download.
    pub photo_type: String,
}
```

In `src/telegram/types.rs`, update the media re-export line:

```rust
pub use media::{MediaDownload, MediaFilter, MediaType, SizeCandidate};
```

- [ ] **Step 4.4: Run tests to verify they pass**

Run: `cargo test media`
Expected: PASS.

- [ ] **Step 4.5: Commit**

```bash
cargo fmt --all
git add src/telegram/types/media.rs src/telegram/types.rs
git commit -m "feat: add MediaDownload and SizeCandidate domain types"
```

---

### Task 5: Size selection in converters.rs

**Files:**
- Modify: `src/telegram/converters.rs`

Two functions: a pure selector (unit-tested) and a grammers→SizeCandidate extractor (untestable glue — grammers `PhotoSize` has private fields and cannot be constructed in tests; keep it trivial).

- [ ] **Step 5.1: Write the failing tests**

`src/telegram/converters.rs` currently has no `#[cfg(test)]` block — add one at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::types::SizeCandidate;

    fn candidate(width: u32, height: u32, size_bytes: u64, tag: &str) -> SizeCandidate {
        SizeCandidate {
            width,
            height,
            size_bytes,
            photo_type: tag.to_string(),
        }
    }

    #[test]
    fn selects_smallest_candidate_that_satisfies_max_dimension() {
        let candidates = vec![
            candidate(320, 180, 10_000, "m"),
            candidate(1280, 720, 100_000, "x"),
            candidate(2560, 1440, 400_000, "y"),
        ];
        let selected = select_size_candidate(&candidates, 1280).unwrap();
        assert_eq!(selected.photo_type, "x");
    }

    #[test]
    fn falls_back_to_largest_when_none_satisfies() {
        let candidates = vec![
            candidate(320, 180, 10_000, "m"),
            candidate(800, 450, 40_000, "x"),
        ];
        let selected = select_size_candidate(&candidates, 1280).unwrap();
        assert_eq!(selected.photo_type, "x");
    }

    #[test]
    fn empty_candidates_returns_none() {
        assert!(select_size_candidate(&[], 1280).is_none());
    }

    #[test]
    fn longest_side_is_what_counts() {
        // 720x1280 portrait qualifies for max_dimension 1280 via its height.
        let candidates = vec![
            candidate(720, 1280, 90_000, "x"),
            candidate(1440, 2560, 300_000, "y"),
        ];
        let selected = select_size_candidate(&candidates, 1280).unwrap();
        assert_eq!(selected.photo_type, "x");
    }
}
```

- [ ] **Step 5.2: Run tests to verify they fail**

Run: `cargo test converters`
Expected: FAIL — `cannot find function select_size_candidate` (compile error).

- [ ] **Step 5.3: Implement the selector and extractor**

In `src/telegram/converters.rs`:

Update imports:

```rust
use crate::telegram::types::{
    Channel, ChannelId, ChannelName, MediaFilter, MediaType, Message, MessageId, SizeCandidate,
    UserId, Username,
};
use grammers_client::media::{Document, Media, PhotoSize};
```

Add the functions:

```rust
/// Pick the size variant to download: the smallest whose longest side is at
/// least `max_dimension` (no point downloading more pixels than will be
/// returned), or the largest available when none qualifies.
pub fn select_size_candidate(
    candidates: &[SizeCandidate],
    max_dimension: u32,
) -> Option<SizeCandidate> {
    candidates
        .iter()
        .filter(|c| c.width.max(c.height) >= max_dimension)
        .min_by_key(|c| c.width.max(c.height))
        .or_else(|| candidates.iter().max_by_key(|c| c.width.max(c.height)))
        .cloned()
}

/// Extract downloadable size candidates from grammers photo/document thumbs.
///
/// `Stripped` and `Path` variants are tiny inline previews / vector outlines,
/// not photo content; `Empty` is unavailable. All three are skipped.
pub fn size_candidates(thumbs: &[PhotoSize]) -> Vec<SizeCandidate> {
    thumbs
        .iter()
        .filter_map(|thumb| match thumb {
            PhotoSize::Size(s) => Some(SizeCandidate {
                width: s.width.max(0) as u32,
                height: s.height.max(0) as u32,
                size_bytes: s.size.max(0) as u64,
                photo_type: thumb.photo_type(),
            }),
            PhotoSize::Cached(s) => Some(SizeCandidate {
                width: s.width.max(0) as u32,
                height: s.height.max(0) as u32,
                size_bytes: s.bytes.len() as u64,
                photo_type: thumb.photo_type(),
            }),
            PhotoSize::Progressive(s) => Some(SizeCandidate {
                width: s.width.max(0) as u32,
                height: s.height.max(0) as u32,
                size_bytes: thumb.size() as u64,
                photo_type: thumb.photo_type(),
            }),
            PhotoSize::Empty(_) | PhotoSize::Stripped(_) | PhotoSize::Path(_) => None,
        })
        .collect()
}
```

Note: `Size`/`CachedSize`/`ProgressiveSize` expose `pub width: i32, pub height: i32` (and `Size` exposes `pub size: i32`); `photo_type()` is the accessor on the enum. Verified against the grammers checkout at `~/.cargo/git/checkouts/grammers-*/*/grammers-client/src/media/photo_sizes.rs`.

- [ ] **Step 5.4: Run tests to verify they pass**

Run: `cargo test converters`
Expected: PASS (4 new tests).

- [ ] **Step 5.5: Commit**

```bash
cargo fmt --all
git add src/telegram/converters.rs
git commit -m "feat: add size-candidate extraction and selection for media download"
```

---

### Task 6: Pure image pipeline

**Files:**
- Create: `src/mcp/tools/image.rs`
- Modify: `src/mcp/tools.rs` (module declaration)
- Modify: `src/test_helpers.rs` (JPEG fixture)

- [ ] **Step 6.1: Add the JPEG fixture to test_helpers**

Add to `src/test_helpers.rs`:

```rust
/// Generate an in-memory JPEG with a noisy gradient (compresses poorly, so
/// payload-cap tests can trigger the shrink loop with a small cap).
pub fn create_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x % 256) as u8,
            (y % 256) as u8,
            ((x * 7 + y * 13) % 256) as u8,
        ])
    });
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut buf, 90,
        ))
        .expect("test JPEG encoding cannot fail");
    buf.into_inner()
}
```

- [ ] **Step 6.2: Write the failing tests**

Create `src/mcp/tools/image.rs` with the test module first (the implementation comes in Step 6.4 — the file needs the signatures to compile, so write tests against the API):

```rust
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
}
```

Declare the module in `src/mcp/tools.rs`:

```rust
pub mod helpers;
pub mod image;
pub mod types;
```

- [ ] **Step 6.3: Run tests to verify they fail**

Run: `cargo test mcp::tools::image`
Expected: FAIL — `cannot find function process_image` (compile error).

- [ ] **Step 6.4: Implement the pipeline**

Prepend to `src/mcp/tools/image.rs` (above the test module):

```rust
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
fn process_image_with_cap(
    bytes: &[u8],
    max_dimension: u32,
    max_base64_len: usize,
) -> Result<ProcessedImage, Error> {
    let decoded = image::load_from_memory(bytes)
        .map_err(|e| Error::DownloadFailed(format!("failed to decode image: {e}")))?;

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
        let ratio = (max_base64_len as f64 / encoded.len() as f64).sqrt().min(0.9);
        target = ((f64::from(target)) * ratio).floor().max(1.0) as u32;
    }

    Err(Error::DownloadFailed(format!(
        "image could not be reduced below the {max_base64_len}-byte payload cap"
    )))
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
```

Note: the test calls `process_image_with_cap` — it is private but the test module is a child module, so access works.

- [ ] **Step 6.5: Run tests to verify they pass**

Run: `cargo test mcp::tools::image`
Expected: PASS (5 tests).

- [ ] **Step 6.6: Commit**

```bash
cargo fmt --all
git add src/mcp/tools/image.rs src/mcp/tools.rs src/test_helpers.rs
git commit -m "feat: add pure image pipeline (downscale, JPEG re-encode, payload cap)"
```

---

### Task 7: Trait method + client implementation

**Files:**
- Modify: `src/telegram/trait_def.rs`
- Modify: `src/telegram/client.rs`
- Test: `src/telegram/tests/client_tests.rs` (mock conformance)

- [ ] **Step 7.1: Write the failing mock-conformance test**

Add to `src/telegram/tests/client_tests.rs`:

```rust
#[tokio::test]
async fn mock_download_message_media_returns_media_download() {
    use crate::telegram::types::{MediaDownload, MediaType};

    let mut mock = MockTelegramClientTrait::new();
    mock.expect_download_message_media()
        .withf(|channel_ref, msg_id, max_dim| {
            channel_ref == "news" && *msg_id == 42 && *max_dim == 1280
        })
        .return_once(|_, _, _| {
            Ok(MediaDownload {
                bytes: vec![0xff, 0xd8, 0xff],
                media_type: MediaType::Photo,
                is_thumbnail: false,
                caption: None,
                width: Some(1280),
                height: Some(720),
                source_size_bytes: 3,
            })
        });

    let result = mock.download_message_media("news", 42, 1280).await.unwrap();
    assert_eq!(result.media_type, MediaType::Photo);
    assert_eq!(result.bytes.len(), 3);
}
```

(Match the file's existing import style — it already imports `MockTelegramClientTrait`.)

- [ ] **Step 7.2: Run test to verify it fails**

Run: `cargo test client_tests`
Expected: FAIL — `no method named expect_download_message_media` (compile error).

- [ ] **Step 7.3: Add the trait method**

In `src/telegram/trait_def.rs`, update the imports and add the method:

```rust
use crate::telegram::types::{
    Channel, HistoryParams, MediaDownload, Message, SearchParams, SearchResult,
};
```

```rust
/// Download the visual media of a message: the photo itself, or the
/// server-side thumbnail for video-like media (video, animation, video note).
///
/// `max_dimension` is a size-selection hint: the smallest server-side size
/// whose longest side is at least `max_dimension` is downloaded (the largest
/// available if none qualifies). Exact downscaling happens in the MCP layer.
async fn download_message_media(
    &self,
    channel_ref: &str,
    message_id: i32,
    max_dimension: u32,
) -> Result<MediaDownload, Error>;
```

mockall regenerates `MockTelegramClientTrait` automatically. This breaks compilation of `TelegramClient` until Step 7.4 — that's expected; do both before running tests.

- [ ] **Step 7.4: Extract `resolve_peer` and implement the client method**

In `src/telegram/client.rs`:

1. Extract the peer-resolution block from `get_message_by_id` (the `let peer = if let Ok(id) = channel_ref.parse::<i64>() { ... } else { ... };` block, currently lines ~566–600) into a private helper on `impl TelegramClient` (NOT in the trait):

```rust
/// Resolve a channel reference (numeric ID via dialog walk, or username) to a Peer.
///
/// Extracted from get_message_by_id so download_message_media shares the
/// exact same resolution semantics and timeout budget.
async fn resolve_peer(&self, channel_ref: &str) -> Result<grammers_client::peer::Peer, Error> {
    if let Ok(id) = channel_ref.parse::<i64>() {
        // Numeric ID — search through dialogs
        let found = with_timeout("iter_dialogs", self.timeouts.resolve_secs, async {
            let mut dialogs = self.client.iter_dialogs();
            while let Some(dialog) = dialogs.next().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to iterate dialogs in resolve_peer");
                Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
            })? {
                if dialog.peer().id().bare_id() == id {
                    return Ok(Some(dialog.peer().clone()));
                }
            }
            Ok(None)
        })
        .await?;

        found.ok_or_else(|| {
            tracing::warn!(id, "Channel not found in dialogs by ID");
            Error::InvalidInput(format!("Channel not found: {}", channel_ref))
        })
    } else {
        // Username — resolve directly
        let username = channel_ref.strip_prefix('@').unwrap_or(channel_ref);
        with_timeout("resolve_username", self.timeouts.resolve_secs, async {
            self.client.resolve_username(username).await.map_err(|e| {
                tracing::error!(username = %username, error = %e, "Failed to resolve username");
                Error::TelegramApi(format!("Failed to resolve username: {}", e))
            })
        })
        .await?
        .ok_or_else(|| {
            tracing::warn!(username = %username, "Username not found");
            Error::InvalidInput(format!("Channel not found: {}", channel_ref))
        })
    }
}
```

Replace the inlined block in `get_message_by_id` with `let peer = self.resolve_peer(channel_ref).await?;` (keep its empty-ref guard). The existing `get_message_by_id` tests in `src/mcp/tests/message_by_link.rs` are the safety net for this refactor.

2. Add imports to `client.rs`:

```rust
use crate::telegram::converters::{
    convert_media_filter, convert_media_to_type, convert_message, convert_peer_to_channel,
    matches_media_filter, select_size_candidate, size_candidates,
};
use crate::telegram::types::{
    HistoryParams, MediaDownload, MediaType, QueryMetadata, SearchParams, SearchResult,
};
use grammers_client::media::Media;
```

3. Implement the trait method inside `impl TelegramClientTrait for TelegramClient` (after `get_message_by_id`):

```rust
async fn download_message_media(
    &self,
    channel_ref: &str,
    message_id: i32,
    max_dimension: u32,
) -> Result<MediaDownload, Error> {
    /// Spec limit: never pull more than 20 MB over the network.
    const MAX_DOWNLOAD_BYTES: u64 = 20 * 1024 * 1024;

    if channel_ref.is_empty() {
        return Err(Error::InvalidInput(
            "Channel reference cannot be empty".to_string(),
        ));
    }

    let peer = self.resolve_peer(channel_ref).await?;
    let peer_ref = peer
        .to_ref()
        .await
        .ok_or_else(|| Error::TelegramApi("Failed to convert peer to PeerRef".to_string()))?;

    let messages = with_timeout("get_messages_by_id", self.timeouts.history_secs, async {
        self.client
            .get_messages_by_id(peer_ref, &[message_id])
            .await
            .map_err(|e| {
                tracing::error!(
                    channel_ref = %channel_ref,
                    message_id,
                    error = %e,
                    "Failed to get message for media download"
                );
                Error::TelegramApi(format!("Failed to get message: {}", e))
            })
    })
    .await?;

    let msg = messages.into_iter().next().flatten().ok_or_else(|| {
        Error::InvalidInput(format!(
            "Message {} not found in channel {}",
            message_id, channel_ref
        ))
    })?;

    let media = msg.media().ok_or_else(|| Error::NoVisualMedia {
        media_type: "none".to_string(),
    })?;
    let media_type = convert_media_to_type(&media);

    // Photos are downloaded directly; video-like media contributes only its
    // server-side thumbnail (the spec forbids full video downloads).
    let (thumbs, is_thumbnail) = match &media {
        Media::Photo(photo) => (photo.thumbs(), false),
        Media::Document(doc)
            if matches!(
                media_type,
                MediaType::Video | MediaType::Animation | MediaType::VideoNote
            ) =>
        {
            (doc.thumbs(), true)
        }
        _ => {
            return Err(Error::NoVisualMedia {
                media_type: format!("{:?}", media_type).to_lowercase(),
            });
        }
    };

    let candidates = size_candidates(&thumbs);
    let selected = select_size_candidate(&candidates, max_dimension).ok_or_else(|| {
        Error::DownloadFailed("no downloadable size variant available".to_string())
    })?;

    if selected.size_bytes > MAX_DOWNLOAD_BYTES {
        return Err(Error::MediaTooLarge {
            size_bytes: selected.size_bytes,
            max_bytes: MAX_DOWNLOAD_BYTES,
        });
    }

    let photo_size = thumbs
        .iter()
        .find(|t| t.photo_type() == selected.photo_type)
        .ok_or_else(|| {
            Error::DownloadFailed("selected size variant disappeared".to_string())
        })?;

    let bytes = with_timeout("download_media", self.timeouts.download_secs, async {
        let mut data: Vec<u8> = Vec::new();
        let mut download = self.client.iter_download(photo_size);
        loop {
            match download.next().await {
                Ok(Some(chunk)) => {
                    data.extend_from_slice(&chunk);
                    // Reported sizes are untrusted input; re-check while streaming.
                    if data.len() as u64 > MAX_DOWNLOAD_BYTES {
                        return Err(Error::MediaTooLarge {
                            size_bytes: data.len() as u64,
                            max_bytes: MAX_DOWNLOAD_BYTES,
                        });
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::error!(
                        channel_ref = %channel_ref,
                        message_id,
                        error = %e,
                        "Media download failed"
                    );
                    return Err(Error::DownloadFailed(format!("download failed: {}", e)));
                }
            }
        }
        Ok(data)
    })
    .await?;

    let caption = match msg.text() {
        "" => None,
        text => Some(text.to_string()),
    };

    tracing::info!(
        channel_ref = %channel_ref,
        message_id,
        media_type = ?media_type,
        is_thumbnail,
        selected_type = %selected.photo_type,
        bytes = bytes.len(),
        "Media downloaded"
    );

    Ok(MediaDownload {
        bytes,
        media_type,
        is_thumbnail,
        caption,
        width: Some(selected.width),
        height: Some(selected.height),
        source_size_bytes: selected.size_bytes,
    })
}
```

- [ ] **Step 7.5: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS — new mock test passes, all existing tests (especially `message_by_link`) still pass after the `resolve_peer` extraction.

- [ ] **Step 7.6: Commit**

```bash
cargo fmt --all
git add src/telegram/trait_def.rs src/telegram/client.rs src/telegram/tests/client_tests.rs
git commit -m "feat: add download_message_media to TelegramClientTrait and client"
```

---

### Task 8: Request and response types

**Files:**
- Modify: `src/mcp/tools/types/requests.rs`
- Modify: `src/mcp/tools/types/responses.rs`

- [ ] **Step 8.1: Write the failing tests**

Add to the test module in `src/mcp/tools/types/responses.rs`:

```rust
#[test]
fn get_message_media_response_serializes() {
    use crate::telegram::types::MediaType;

    let response = GetMessageMediaResponse {
        channel_id: "news".to_string(),
        message_id: 42,
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: Some("benchmark table".to_string()),
        original_width: Some(2560),
        original_height: Some(1440),
        original_size_bytes: 400_000,
        returned_width: 1280,
        returned_height: 720,
        returned_size_bytes: 150_000,
        mime_type: "image/jpeg".to_string(),
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"media_type\":\"photo\""));
    assert!(json.contains("\"is_thumbnail\":false"));
    assert!(json.contains("benchmark table"));
}
```

Request deserialization tests — find the existing request-deserialization test location (`cargo test requests` / the serde_helpers tests in `src/mcp/tools/types/`); add wherever `SearchRequest`-style deserialization tests live (if none exist for requests.rs, add a `#[cfg(test)] mod tests` at the bottom of `requests.rs`):

```rust
#[test]
fn get_message_media_request_deserializes_with_flexible_scalars() {
    // message_id arrives as a numeric string; channel_id as a number.
    let json = r#"{"channel_id": 123456, "message_id": "42", "max_dimension": "640"}"#;
    let request: GetMessageMediaRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.channel_id, "123456");
    assert_eq!(request.message_id, 42);
    assert_eq!(request.max_dimension, Some(640));
}

#[test]
fn get_message_media_request_max_dimension_defaults_to_none() {
    let json = r#"{"channel_id": "news", "message_id": 42}"#;
    let request: GetMessageMediaRequest = serde_json::from_str(json).unwrap();
    assert_eq!(request.max_dimension, None);
}
```

- [ ] **Step 8.2: Run tests to verify they fail**

Run: `cargo test types`
Expected: FAIL — unknown structs (compile error).

- [ ] **Step 8.3: Add the types**

In `src/mcp/tools/types/requests.rs` (after `GetMessageByLinkRequest`):

```rust
/// Request for get_message_media tool
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetMessageMediaRequest {
    #[schemars(description = "Channel ID or username (required)")]
    #[serde(deserialize_with = "flexible_string")]
    pub channel_id: String,

    #[schemars(description = "Message ID within the channel")]
    #[serde(deserialize_with = "flexible_i64")]
    pub message_id: i64,

    #[schemars(
        description = "Longest image side in pixels after downscaling (default: 1280, clamped to 64-2048)"
    )]
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub max_dimension: Option<u32>,
}
```

In `src/mcp/tools/types/responses.rs` (imports already include `schemars::JsonSchema`, `serde::{Deserialize, Serialize}`; add `MediaType` to the `crate::telegram::types` import):

```rust
/// Metadata text block accompanying the get_message_media image block
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetMessageMediaResponse {
    #[schemars(description = "Channel the message belongs to (as passed in the request)")]
    pub channel_id: String,

    #[schemars(description = "Message ID")]
    pub message_id: i64,

    #[schemars(description = "Media type of the source message (photo, video, ...)")]
    pub media_type: MediaType,

    #[schemars(description = "True when the image is a video's thumbnail, not the media itself")]
    pub is_thumbnail: bool,

    #[schemars(description = "Message caption, if any")]
    pub caption: Option<String>,

    #[schemars(description = "Pixel width of the downloaded source variant")]
    pub original_width: Option<u32>,

    #[schemars(description = "Pixel height of the downloaded source variant")]
    pub original_height: Option<u32>,

    #[schemars(description = "Byte size of the downloaded source variant")]
    pub original_size_bytes: u64,

    #[schemars(description = "Pixel width of the returned image")]
    pub returned_width: u32,

    #[schemars(description = "Pixel height of the returned image")]
    pub returned_height: u32,

    #[schemars(description = "Encoded JPEG size in bytes (before base64 expansion)")]
    pub returned_size_bytes: usize,

    #[schemars(description = "Always image/jpeg")]
    pub mime_type: String,
}
```

Check `src/mcp/tools/types.rs` re-exports both new types (it re-exports `requests::*` / `responses::*` or names them explicitly — follow whichever pattern is there).

- [ ] **Step 8.4: Run tests to verify they pass**

Run: `cargo test types`
Expected: PASS.

- [ ] **Step 8.5: Commit**

```bash
cargo fmt --all
git add src/mcp/tools/types/requests.rs src/mcp/tools/types/responses.rs src/mcp/tools/types.rs
git commit -m "feat: add GetMessageMediaRequest and GetMessageMediaResponse types"
```

---

### Task 9: The tool handler

**Files:**
- Modify: `src/mcp/server.rs`
- Create: `src/mcp/tests/media.rs`
- Modify: `src/mcp/tests.rs` (register the test module)

- [ ] **Step 9.1: Write the failing tests**

Register in `src/mcp/tests.rs` (alphabetical, after `links`):

```rust
#[path = "tests/media.rs"]
mod media;
```

Create `src/mcp/tests/media.rs`:

```rust
//! Tests for get_message_media tool

use crate::error::Error;
use crate::mcp::server::McpServer;
use crate::mcp::tools::{GetMessageMediaRequest, GetMessageMediaResponse};
use crate::rate_limiter::MockRateLimiterTrait;
use crate::telegram::MockTelegramClientTrait;
use crate::telegram::types::{MediaDownload, MediaType};
use crate::test_helpers::create_test_jpeg;
use base64::Engine as _;
use mockall::predicate::eq;
use rmcp::handler::server::common::RequestId;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{NumberOrString, RawContent};
use std::sync::Arc;

fn photo_download(width: u32, height: u32) -> MediaDownload {
    let bytes = create_test_jpeg(width, height);
    let source_size_bytes = bytes.len() as u64;
    MediaDownload {
        bytes,
        media_type: MediaType::Photo,
        is_thumbnail: false,
        caption: Some("benchmark chart".to_string()),
        width: Some(width),
        height: Some(height),
        source_size_bytes,
    }
}

fn request(channel: &str, message_id: i64, max_dimension: Option<u32>) -> GetMessageMediaRequest {
    GetMessageMediaRequest {
        channel_id: channel.to_string(),
        message_id,
        max_dimension,
    }
}

#[tokio::test]
async fn photo_returns_image_and_metadata_blocks() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .withf(|channel, msg_id, max_dim| channel == "news" && *msg_id == 42 && *max_dim == 1280)
        .return_once(|_, _, _| Ok(photo_download(200, 100)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().with(eq(5)).returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 42, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let call_result = result.expect("tool should succeed");
    assert_eq!(call_result.content.len(), 2);

    let RawContent::Image(img) = &call_result.content[0].raw else {
        panic!("first content block must be an image");
    };
    assert_eq!(img.mime_type, "image/jpeg");
    let jpeg = base64::engine::general_purpose::STANDARD
        .decode(&img.data)
        .expect("image data must be valid base64");
    let decoded = image::load_from_memory(&jpeg).expect("must be a decodable JPEG");
    assert_eq!(decoded.width(), 200); // source smaller than max_dimension: no upscale

    let RawContent::Text(text) = &call_result.content[1].raw else {
        panic!("second content block must be text");
    };
    let metadata: GetMessageMediaResponse = serde_json::from_str(&text.text).unwrap();
    assert_eq!(metadata.media_type, MediaType::Photo);
    assert!(!metadata.is_thumbnail);
    assert_eq!(metadata.caption.as_deref(), Some("benchmark chart"));
    assert_eq!(metadata.mime_type, "image/jpeg");
    assert_eq!(metadata.returned_width, 200);
    assert_eq!(metadata.returned_height, 100);
}

#[tokio::test]
async fn video_thumbnail_sets_is_thumbnail() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_download_message_media().return_once(|_, _, _| {
        let bytes = create_test_jpeg(320, 180);
        let source_size_bytes = bytes.len() as u64;
        Ok(MediaDownload {
            bytes,
            media_type: MediaType::Video,
            is_thumbnail: true,
            caption: None,
            width: Some(320),
            height: Some(180),
            source_size_bytes,
        })
    });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 7, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let call_result = result.expect("tool should succeed");
    let RawContent::Text(text) = &call_result.content[1].raw else {
        panic!("second content block must be text");
    };
    let metadata: GetMessageMediaResponse = serde_json::from_str(&text.text).unwrap();
    assert_eq!(metadata.media_type, MediaType::Video);
    assert!(metadata.is_thumbnail);
    assert!(metadata.caption.is_none());
}

#[tokio::test]
async fn no_visual_media_returns_structured_error() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_download_message_media().return_once(|_, _, _| {
        Err(Error::NoVisualMedia {
            media_type: "poll".to_string(),
        })
    });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 8, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must be an error");
    assert!(error.contains("no visual media"));
    assert!(error.contains("poll"));
}

#[tokio::test]
async fn oversize_media_is_rejected() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_download_message_media().return_once(|_, _, _| {
        Err(Error::MediaTooLarge {
            size_bytes: 25_000_000,
            max_bytes: 20_971_520,
        })
    });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 9, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must be an error");
    assert!(error.contains("media too large"));
    assert!(error.contains("25000000"));
}

#[tokio::test]
async fn max_dimension_is_clamped_to_2048() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .withf(|_, _, max_dim| *max_dim == 2048)
        .return_once(|_, _, _| Ok(photo_download(64, 64)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 10, Some(5000))),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn configured_media_download_cost_is_charged() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client
        .expect_download_message_media()
        .return_once(|_, _, _| Ok(photo_download(64, 64)));

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().with(eq(9)).returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter))
        .with_media_download_cost(9);
    let result = server
        .get_message_media(
            Parameters(request("news", 11, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn rate_limited_request_never_reaches_telegram() {
    // No expectation on the client mock: a call would panic the test.
    let mock_client = MockTelegramClientTrait::new();

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| {
        Err(Error::RateLimit {
            retry_after_seconds: 3,
        })
    });

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 12, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must be rate limited");
    assert!(error.contains("rate limit exceeded"));
}

#[tokio::test]
async fn corrupt_image_bytes_return_decode_error() {
    let mut mock_client = MockTelegramClientTrait::new();
    mock_client.expect_download_message_media().return_once(|_, _, _| {
        Ok(MediaDownload {
            bytes: vec![0x00; 32],
            media_type: MediaType::Photo,
            is_thumbnail: false,
            caption: None,
            width: None,
            height: None,
            source_size_bytes: 32,
        })
    });

    let mut mock_limiter = MockRateLimiterTrait::new();
    mock_limiter.expect_acquire().returning(|_| Ok(()));

    let server = McpServer::new(Arc::new(mock_client), Arc::new(mock_limiter));
    let result = server
        .get_message_media(
            Parameters(request("news", 13, None)),
            RequestId(NumberOrString::Number(1)),
        )
        .await;

    let error = result.expect_err("must fail to decode");
    assert!(error.contains("decode"));
}
```

- [ ] **Step 9.2: Run tests to verify they fail**

Run: `cargo test mcp::tests::media`
Expected: FAIL — `no method named get_message_media` (compile error).

- [ ] **Step 9.3: Implement server changes**

In `src/mcp/server.rs`:

1. Imports — extend:

```rust
use crate::mcp::tools::image::process_image;
use crate::mcp::tools::{
    BufferedResponseEntry, ChannelsResponse, GenerateLinkRequest, GetChannelInfoRequest,
    GetChannelsRequest, GetLastResponsesRequest, GetMessageByLinkRequest, GetMessageMediaRequest,
    GetMessageMediaResponse, GetRecentMessagesRequest, LastResponsesResponse, MessageLinkResponse,
    OpenMessageRequest, OpenMessageResponse, SearchRequest, StatusResponse, parse_channel_id,
    parse_message_id, parse_optional_channel_id,
};
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, ServerCapabilities,
};
```

2. Struct field + constructor default + builder (mirror `with_observability`):

```rust
pub struct McpServer<T: TelegramClientTrait, R: RateLimiterTrait> {
    telegram_client: Arc<T>,
    rate_limiter: Arc<R>,
    metrics: Arc<SessionMetrics>,
    response_buffer: Arc<ResponseBuffer>,
    slow_write_threshold: Duration,
    media_download_cost: u32,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}
```

In `new()`, add `media_download_cost: 5,` (the config default; overridden from `[rate_limiting]` in main).

```rust
/// Set the rate-limiter cost charged per get_message_media call
/// (`[rate_limiting] media_download_cost`, default 5).
pub fn with_media_download_cost(mut self, cost: u32) -> Self {
    self.media_download_cost = cost;
    self
}
```

3. Generalize `log_tool_outcome` — only the signature line changes:

```rust
fn log_tool_outcome<T>(
    tool: &str,
    request_id: &str,
    started: Instant,
    result: &Result<T, String>,
) {
```

(body unchanged — it only reads the `Err` arm.)

4. The `_impl` method (in the first `impl` block, after `get_last_responses_impl`):

```rust
async fn get_message_media_impl(
    &self,
    request: GetMessageMediaRequest,
) -> Result<CallToolResult, String> {
    const DEFAULT_MAX_DIMENSION: u32 = 1280;
    const MIN_DIMENSION: u32 = 64;
    const MAX_DIMENSION: u32 = 2048;

    let message_id = parse_message_id(request.message_id)?;
    let max_dimension = request
        .max_dimension
        .unwrap_or(DEFAULT_MAX_DIMENSION)
        .clamp(MIN_DIMENSION, MAX_DIMENSION);

    // Media downloads are heavier than searches; charge the configured cost.
    self.rate_limiter
        .acquire(self.media_download_cost)
        .await
        .map_err(|e| e.to_string())?;

    let download = self
        .telegram_client
        .download_message_media(&request.channel_id, message_id.get() as i32, max_dimension)
        .await
        .map_err(|e| e.to_string())?;

    let processed = process_image(&download.bytes, max_dimension).map_err(|e| e.to_string())?;

    let metadata = GetMessageMediaResponse {
        channel_id: request.channel_id.clone(),
        message_id: message_id.get(),
        media_type: download.media_type,
        is_thumbnail: download.is_thumbnail,
        caption: download.caption,
        original_width: download.width,
        original_height: download.height,
        original_size_bytes: download.source_size_bytes,
        returned_width: processed.width,
        returned_height: processed.height,
        returned_size_bytes: processed.encoded_size_bytes,
        mime_type: "image/jpeg".to_string(),
    };

    tracing::info!(
        channel = %request.channel_id,
        message_id = message_id.get(),
        media_type = ?metadata.media_type,
        is_thumbnail = metadata.is_thumbnail,
        returned_bytes = metadata.returned_size_bytes,
        "Message media results"
    );

    let metadata_json = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;

    Ok(CallToolResult::success(vec![
        Content::image(processed.base64_jpeg, "image/jpeg"),
        Content::text(metadata_json),
    ]))
}
```

5. The `#[tool]` wrapper (in the `#[tool_router]` impl block, after `get_last_responses`):

```rust
/// Tool 10: get_message_media - Return a message's photo (or video thumbnail) as an image
#[tool(
    description = "Get a message's photo (or the thumbnail of its video/animation/video note) as an image the model can see, plus a JSON metadata block. Photos are downscaled (max_dimension, default 1280) and re-encoded as JPEG. Heavier than a search: charged media_download_cost rate-limit tokens."
)]
pub async fn get_message_media(
    &self,
    Parameters(request): Parameters<GetMessageMediaRequest>,
    id: RequestId,
) -> Result<CallToolResult, String> {
    let request_id = id.0.to_string();
    let started = Instant::now();
    tracing::info!(
        tool = "get_message_media",
        request_id = %request_id,
        channel_id = %request.channel_id,
        message_id = request.message_id,
        max_dimension = ?request.max_dimension,
        "Tool invocation started"
    );
    let result = self.get_message_media_impl(request).await;
    log_tool_outcome("get_message_media", &request_id, started, &result);
    result
}
```

Why `Result<CallToolResult, String>` is allowed here (the one deviation from the project's `Result<String, String>` convention): rmcp's actual requirement is `IntoCallToolResult`, which `CallToolResult` implements directly and `Result<T, E>` composes; the `Err(String)` arm yields a text block with `is_error: true`, identical to the other tools. See the design spec.

- [ ] **Step 9.4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS — all 8 new media tests, all existing tests.

- [ ] **Step 9.5: Commit**

```bash
cargo fmt --all
git add src/mcp/server.rs src/mcp/tests.rs src/mcp/tests/media.rs
git commit -m "feat: add get_message_media MCP tool (tool 10)"
```

---

### Task 10: ResponseBuffer oversized-payload guard

**Files:**
- Modify: `src/mcp/observability.rs`
- Modify: `src/mcp/server.rs` (two `ResponseBuffer::new` call sites)

- [ ] **Step 10.1: Write the failing tests**

Add to the test module in `src/mcp/observability.rs`:

```rust
#[test]
fn push_replaces_oversized_payload_with_stub() {
    let buffer = ResponseBuffer::new(5, 100);
    buffer.push(BufferedResponse {
        request_id: "1".to_string(),
        tool_name: "get_message_media".to_string(),
        written_at: SystemTime::now(),
        size_bytes: 200,
        payload: "x".repeat(200),
    });

    let entries = buffer.last(None);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].payload, OVERSIZED_PAYLOAD_STUB);
    // size_bytes still reports the real wire size.
    assert_eq!(entries[0].size_bytes, 200);
    // The stub must stay valid JSON so get_last_responses can embed it.
    assert!(serde_json::from_str::<serde_json::Value>(OVERSIZED_PAYLOAD_STUB).is_ok());
}

#[test]
fn push_keeps_payload_at_or_under_threshold() {
    let buffer = ResponseBuffer::new(5, 100);
    buffer.push(BufferedResponse {
        request_id: "1".to_string(),
        tool_name: "search_messages".to_string(),
        written_at: SystemTime::now(),
        size_bytes: 100,
        payload: "y".repeat(100),
    });

    let entries = buffer.last(None);
    assert_eq!(entries[0].payload, "y".repeat(100));
}
```

- [ ] **Step 10.2: Run tests to verify they fail**

Run: `cargo test observability`
Expected: FAIL — `new` takes 1 argument / `OVERSIZED_PAYLOAD_STUB` not found (compile error).

- [ ] **Step 10.3: Implement the guard**

In `src/mcp/observability.rs`:

```rust
/// Payload stored in place of response bodies larger than
/// `[observability] max_buffered_payload_bytes`. Valid JSON so
/// get_last_responses can embed it as-is.
pub const OVERSIZED_PAYLOAD_STUB: &str =
    r#"{"omitted":"payload exceeded max_buffered_payload_bytes"}"#;
```

`ResponseBuffer` — add field and update `new`/`push`:

```rust
pub struct ResponseBuffer {
    capacity: usize,
    max_payload_bytes: usize,
    entries: Mutex<VecDeque<BufferedResponse>>,
}

impl ResponseBuffer {
    pub fn new(capacity: usize, max_payload_bytes: usize) -> Self {
        Self {
            capacity,
            max_payload_bytes,
            entries: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push(&self, mut entry: BufferedResponse) {
        if self.capacity == 0 {
            return;
        }
        if entry.payload.len() > self.max_payload_bytes {
            entry.payload = OVERSIZED_PAYLOAD_STUB.to_string();
        }
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }
    // last/len/is_empty unchanged
}
```

Update every `ResponseBuffer::new(...)` call site:

- `src/mcp/server.rs` `new()`: `ResponseBuffer::new(observability.response_buffer_size, observability.max_buffered_payload_bytes)`
- `src/mcp/server.rs` `with_observability()`: same with `config.…`
- Existing tests in `observability.rs` (`ResponseBuffer::new(10)`, `::new(5)`, `::new(2)` — around lines 424, 539, 639, 650): add `usize::MAX` as the second argument to preserve their semantics.

- [ ] **Step 10.4: Run tests to verify they pass**

Run: `cargo test`
Expected: PASS — 2 new tests, all existing observability/last_responses tests still green.

- [ ] **Step 10.5: Commit**

```bash
cargo fmt --all
git add src/mcp/observability.rs src/mcp/server.rs
git commit -m "feat: stub oversized payloads in the response ring buffer"
```

---

### Task 11: Wire config in main.rs

**Files:**
- Modify: `src/main.rs` (around line 104)

- [ ] **Step 11.1: Wire the cost**

```rust
let server = McpServer::new(Arc::new(telegram_client), Arc::new(rate_limiter))
    .with_observability(&config.observability)
    .with_media_download_cost(config.rate_limiting.media_download_cost);
```

- [ ] **Step 11.2: Verify**

Run: `cargo build && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 11.3: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire media_download_cost from config into the MCP server"
```

---

### Task 12: Documentation updates

**Files:**
- Modify: `README.md`, `CHANGELOG.md`, `CLAUDE.md`, `src/mcp/tools.rs`, `.claude/rules/ast-index.md`, `.claude/skills/project-conventions/SKILL.md`, `docs/tasklist.md`, `docs/memory.md`

- [ ] **Step 12.1: README — MCP Tools Reference**

Add a new numbered subsection for `get_message_media` in the `## MCP Tools Reference` section, matching the existing per-tool format (description, parameters table, example). Parameters table:

```markdown
| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `channel_id` | string | Yes | - | Channel ID or username |
| `message_id` | integer | Yes | - | Message ID within the channel |
| `max_dimension` | integer | No | 1280 | Longest image side in pixels after downscaling (clamped to 64–2048) |
```

Describe: returns an MCP **image content block** (base64 JPEG) plus a JSON metadata text block (`media_type`, `is_thumbnail`, `caption`, original/returned dimensions and sizes); videos/animations/video notes return only their server-side thumbnail with `is_thumbnail: true`; messages without visual media return a structured error; photos over 20 MB are refused; payload capped at ~1.5 MB. Mention the `media_download_cost` rate-limit charge (default 5 tokens vs 1 for searches). Update any "9 tools" / tool-count phrasing in README, and document the new config keys in the config reference section if README has one (`media_download_cost`, `download_secs`, `max_buffered_payload_bytes`).

- [ ] **Step 12.2: CHANGELOG**

Under `## [Unreleased]` add:

```markdown
### Added
- New `get_message_media` tool (tool 10): returns a message's photo — or the server-side thumbnail of its video/animation/video note (`is_thumbnail: true`) — as an MCP image content block (base64 JPEG, quality 80) plus a JSON metadata text block (media type, caption, original/returned dimensions and byte sizes). Images are downscaled so the longest side fits `max_dimension` (default 1280, clamped to 64–2048), with the smallest sufficient server-side size variant chosen before downloading; photos whose selected variant exceeds 20 MB are refused; the base64 payload is capped at ~1.5 MB with automatic further downscaling. Downloads are charged `media_download_cost` rate-limiter tokens (`[rate_limiting]`, default 5) and bounded by a new `download_secs` timeout (`[telegram.timeouts]`, default 120).
- Responses larger than `max_buffered_payload_bytes` (`[observability]`, default 256 KiB) are stored in the `get_last_responses` ring buffer as a stub instead of the full payload, so megabyte-sized image responses don't pin memory or get replayed as text.
```

- [ ] **Step 12.3: Tool-count and convention mentions**

- `CLAUDE.md`: `src/mcp/server.rs (9 tools)` → `(10 tools)`; update the "All 9 tools … All tools return `Result<String, String>`" paragraph to note the exception: *get_message_media returns `Result<CallToolResult, String>` because image content blocks cannot be expressed as a JSON string; rmcp's actual constraint is `IntoCallToolResult`.*
- `src/mcp/tools.rs` header comment: "all 7 MCP tools" → "all 10 MCP tools".
- `.claude/rules/ast-index.md`: "all 8 MCP tools" → "all 10 MCP tools".
- `.claude/skills/project-conventions/SKILL.md`: "All 8 tools" → "All 10 tools"; amend "Return type is always `Result<String, String>`" with the get_message_media exception (same wording as CLAUDE.md).

- [ ] **Step 12.4: Project tracking**

- `docs/tasklist.md`: add Phase 22 row to the progress table: `| 22 | Get Message Media | ✅ Complete | <total test count after this change> | New tool: photo/video-thumbnail retrieval as MCP image blocks |` and bump "Overall Progress" to 22/22. (Get the test count from the `cargo test` summary lines.)
- `docs/memory.md`: append a decision note: get_message_media returns `Result<CallToolResult, String>` — the "all tools return String" rule is a project convention, not an rmcp constraint; rmcp requires only `IntoCallToolResult`. Also note the smallest-sufficient-PhotoSize selection trick and the ResponseBuffer stub guard.

- [ ] **Step 12.5: Commit**

```bash
git add README.md CHANGELOG.md CLAUDE.md src/mcp/tools.rs .claude/rules/ast-index.md .claude/skills/project-conventions/SKILL.md docs/tasklist.md docs/memory.md
git commit -m "docs: document get_message_media tool and new config keys"
```

---

### Task 13: Final verification

- [ ] **Step 13.1: Full pre-merge gate**

Run: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
Expected: all three pass, zero warnings. If fmt fails, run `cargo fmt --all` and amend.

- [ ] **Step 13.2: Config tests serial check**

Run: `cargo test config -- --test-threads=1`
Expected: PASS.

- [ ] **Step 13.3: Push and finish**

Use the superpowers:requesting-code-review / superpowers:finishing-a-development-branch flow: push `feat/get-message-media`, open a PR against `master` (note: the branch builds on `chore/update-dependencies`, which contains the v0.7.0 release and the feature doc — if that branch is merged first, rebase; otherwise the PR will include those commits).

---

## Notes for the implementer

- **grammers is a git dependency** — if `PhotoSize`/`Media` APIs don't match this plan, check the checkout under `~/.cargo/git/checkouts/grammers-*/` first; the field/method names above were verified against revision `fa7692e`.
- **Never `unwrap()`** in production code; `expect()` only in tests.
- **Run `cargo fmt --all` after every code change**, not just `--check`.
- **Tool count drift**: docs variously say 7, 8, and 9 tools today; after this change the number everywhere is **10**.
