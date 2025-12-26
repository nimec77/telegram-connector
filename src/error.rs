use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("telegram API error: {0}")]
    TelegramApi(String),

    #[error("rate limit exceeded, retry after {retry_after_seconds} seconds")]
    RateLimit { retry_after_seconds: u64 },

    #[error("configuration error: {0}")]
    Config(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("MCP protocol error: {0}")]
    Mcp(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_error_display() {
        let error = Error::Auth("invalid credentials".to_string());
        assert_eq!(
            error.to_string(),
            "authentication failed: invalid credentials"
        );
    }

    #[test]
    fn test_telegram_api_error_display() {
        let error = Error::TelegramApi("flood wait".to_string());
        assert_eq!(error.to_string(), "telegram API error: flood wait");
    }

    #[test]
    fn test_rate_limit_error_display() {
        let error = Error::RateLimit {
            retry_after_seconds: 5,
        };
        assert_eq!(
            error.to_string(),
            "rate limit exceeded, retry after 5 seconds"
        );
    }

    #[test]
    fn test_config_error_display() {
        let error = Error::Config("missing api_id".to_string());
        assert_eq!(error.to_string(), "configuration error: missing api_id");
    }

    #[test]
    fn test_network_error_display() {
        let error = Error::Network("connection timeout".to_string());
        assert_eq!(error.to_string(), "network error: connection timeout");
    }

    #[test]
    fn test_mcp_error_display() {
        let error = Error::Mcp("invalid request".to_string());
        assert_eq!(error.to_string(), "MCP protocol error: invalid request");
    }

    #[test]
    fn test_error_debug_format() {
        let error = Error::Auth("test".to_string());
        let debug_output = format!("{:?}", error);
        assert!(debug_output.contains("Auth"));
        assert!(debug_output.contains("test"));
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn test_invalid_input_error_display() {
        let error = Error::InvalidInput("Channel ID must be positive".to_string());
        assert_eq!(
            error.to_string(),
            "invalid input: Channel ID must be positive"
        );
    }
}
