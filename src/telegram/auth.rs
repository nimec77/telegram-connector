//! Interactive setup workflow; never call while stdout carries MCP frames.

use crate::error::Error;
use crate::logging::redact_phone;
use crate::telegram::client::TelegramClient;
use dialoguer::{Input, Password};

/// Interactive authentication flow for Telegram
///
/// This prompts the user for:
/// - Authentication code (sent to Telegram app)
/// - 2FA password (if enabled on account)
///
/// Returns Ok(()) if authentication succeeds. This sends a login code and may
/// authenticate/update the SQLite session. Human input waits are unbounded;
/// request-code, sign-in and password RPCs each use the resolve timeout. Prompts
/// can write terminal output, including a supplied 2FA hint. Account tool charging
/// is not applied to setup.
///
/// # Errors
/// Returns typed timeouts or redacted authentication failures for login RPCs,
/// and authentication errors for terminal input failure. Passwords, login codes
/// and challenge tokens are not retained in upstream error chains.
///
/// # Manual testing
///
/// Requires a real Telegram client and credentials:
/// 1. `cargo run --bin telegram-mcp -- --setup`
/// 2. Enter phone number when prompted
/// 3. Enter the code received on Telegram
/// 4. If 2FA is enabled, enter password
/// 5. Verify session is saved to `~/.config/telegram-connector/session.db`
pub async fn interactive_auth(
    client: &TelegramClient,
    phone: &str,
    api_hash: &str,
) -> Result<(), Error> {
    // Request login code
    tracing::info!("Requesting login code for phone: {}", redact_phone(phone));
    let token = client.request_login_code(phone, api_hash).await?;

    // Prompt for code
    let code: String = Input::new()
        .with_prompt("Enter the code you received in Telegram")
        .interact_text()
        .map_err(|e| Error::Auth(format!("Failed to read input: {}", e)))?;

    // Sign in with code
    match client.sign_in(&token, &code).await? {
        None => {
            tracing::info!("Successfully authenticated");
            Ok(())
        }
        Some(password_token) => {
            // 2FA is enabled, prompt for password
            if let Some(hint) = password_token.hint() {
                println!("2FA enabled. Password hint: {}", hint);
            }

            let password = Password::new()
                .with_prompt("Enter your 2FA password")
                .interact()
                .map_err(|e| Error::Auth(format!("Failed to read password: {}", e)))?;

            client
                .check_password(password_token, password.trim())
                .await?;

            tracing::info!("Successfully authenticated with 2FA");
            Ok(())
        }
    }
}
