use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub search: SearchConfig,
    pub rate_limiting: RateLimitConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    pub api_id: i32,
    pub api_hash: String,
    pub phone_number: String,
    pub session_file: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub default_hours_back: u32,
    pub max_results_default: u32,
    pub max_results_limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub max_tokens: u32,
    pub refill_rate: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        todo!("Load configuration from file - Phase 3")
    }
}
