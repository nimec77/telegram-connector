use crate::error::Error;
use crate::telegram::client::TelegramClient;
use dialoguer::{Input, Password};
use grammers_client::SignInError;

/// Interactive authentication flow for Telegram
///
/// This prompts the user for:
/// - Authentication code (sent to Telegram app)
/// - 2FA password (if enabled on account)
///
/// Returns Ok(()) if authentication succeeds.
pub async fn interactive_auth(
    client: &TelegramClient,
    phone: &str,
    api_hash: &str,
) -> Result<(), Error> {
    // Request login code
    tracing::info!(
        "Requesting login code for phone: {}...[redacted]",
        &phone[..4]
    );
    let token = client.request_login_code(phone, api_hash).await?;

    // Prompt for code
    let code: String = Input::new()
        .with_prompt("Enter the code you received in Telegram")
        .interact_text()
        .map_err(|e| Error::Auth(format!("Failed to read input: {}", e)))?;

    // Sign in with code
    match client.client().sign_in(&token, &code).await {
        Ok(_) => {
            tracing::info!("Successfully authenticated");
            Ok(())
        }
        Err(SignInError::PasswordRequired(password_token)) => {
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
        Err(e) => Err(Error::Auth(format!("Sign in failed: {}", e))),
    }
}

#[cfg(test)]
mod tests {
    // Note: Authentication tests require a real Telegram client
    // and are tested manually or via integration tests with real credentials
    //
    // To test manually:
    // 1. cargo run --bin telegram-mcp -- --setup
    // 2. Enter phone number when prompted
    // 3. Enter the code received on Telegram
    // 4. If 2FA is enabled, enter password
    // 5. Verify session is saved to ~/.config/telegram-connector/session.db

    #[test]
    fn test_auth_module_compiles() {
        // Placeholder test to verify module compiles
        // Real authentication tests require Telegram API access
    }
}
