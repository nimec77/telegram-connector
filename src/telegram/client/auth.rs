//! Interactive login: request code, sign in, 2FA password.
//!
//! Unit of `client` (LM-2).

use super::*;

impl TelegramClient {
    /// Request login code for authentication
    pub async fn request_login_code(
        &self,
        phone: &str,
        api_hash: &str,
    ) -> Result<grammers_client::client::LoginToken, Error> {
        self.client
            .request_login_code(phone, api_hash)
            .await
            .map_err(|e| Error::Auth(format!("Failed to request login code: {}", e)))
    }

    /// Sign in with the received code
    pub async fn sign_in(
        &self,
        token: &grammers_client::client::LoginToken,
        code: &str,
    ) -> Result<(), Error> {
        match self.client.sign_in(token, code).await {
            Ok(_user) => {
                tracing::info!("Successfully signed in");
                Ok(())
            }
            Err(grammers_client::SignInError::PasswordRequired(password_token)) => {
                Err(Error::Auth(format!(
                    "2FA password required (hint: {:?})",
                    password_token.hint()
                )))
            }
            Err(e) => Err(Error::Auth(format!("Sign in failed: {}", e))),
        }
    }

    /// Sign in with 2FA password
    pub async fn check_password(
        &self,
        password_token: grammers_client::client::PasswordToken,
        password: &str,
    ) -> Result<(), Error> {
        self.client
            .check_password(password_token, password.as_bytes())
            .await
            .map_err(|e| Error::Auth(format!("2FA verification failed: {}", e)))?;
        tracing::info!("Successfully signed in with 2FA");
        Ok(())
    }
}
