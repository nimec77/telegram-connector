use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("telegram API error: {0}")]
    TelegramApi(String),

    #[error("rate limit exceeded")]
    RateLimit,

    #[error("configuration error: {0}")]
    Config(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("MCP protocol error: {0}")]
    Mcp(String),
}
