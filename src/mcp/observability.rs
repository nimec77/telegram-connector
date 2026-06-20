//! Transport-layer observability: session metrics, response ring buffer, and an
//! instrumented stdio transport decorator.
//!
//! Built after the 2026-06-12 incident (`docs/connetion-issue.md`): a tool response
//! was produced but lost between connector stdout and the client, and the logs could
//! not prove delivery. These types log every actual stdout write (request id, payload
//! size, write+flush duration), warn on blocked writes, and emit a session summary
//! when the input stream ends.

mod buffer;
mod metrics;
mod transport;

pub use buffer::{
    BufferedResponse, GET_LAST_RESPONSES_TOOL, OVERSIZED_PAYLOAD_STUB, ResponseBuffer,
};
pub use metrics::{InFlightRequest, SessionMetrics};
pub use transport::InstrumentedTransport;
