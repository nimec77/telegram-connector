//! Interactive login: request code, sign in, 2FA password.
//!
//! Unit of `client` (LM-2).

use super::TelegramClient;
use crate::error::Error;
use crate::telegram::timeout::with_timeout;

use grammers_client::SignInError;
use grammers_client::client::{LoginToken, PasswordToken};

impl TelegramClient {
    /// Request a login code within the configured resolve deadline.
    ///
    /// # Errors
    /// Returns a typed timeout or a redacted authentication failure.
    pub async fn request_login_code(
        &self,
        phone: &str,
        api_hash: &str,
    ) -> Result<LoginToken, Error> {
        with_timeout("request_login_code", self.timeouts.resolve_secs, async {
            self.client
                .request_login_code(phone, api_hash)
                .await
                .map_err(|_| Error::Auth("Failed to request login code".into()))
        })
        .await
    }

    /// Sign in with a 2FA password within the configured resolve deadline.
    ///
    /// # Errors
    /// Returns a typed timeout or a redacted authentication failure. No password,
    /// hint, challenge token, or upstream error payload is retained.
    pub async fn check_password(
        &self,
        password_token: PasswordToken,
        password: &str,
    ) -> Result<(), Error> {
        with_timeout("check_password", self.timeouts.resolve_secs, async {
            self.client
                .check_password(password_token, password.as_bytes())
                .await
                .map_err(|_| Error::Auth("2FA verification failed".into()))?;
            Ok(())
        })
        .await?;
        tracing::info!("Successfully signed in with 2FA");
        Ok(())
    }

    /// `Some` carries the 2FA challenge, `None` means authentication succeeded.
    /// This deadline starts after interactive code entry has completed.
    pub(crate) async fn sign_in(
        &self,
        token: &LoginToken,
        code: &str,
    ) -> Result<Option<PasswordToken>, Error> {
        code_sign_in_with_timeout(self.timeouts.resolve_secs, self.client.sign_in(token, code))
            .await
    }
}

/// Keep the challenge as data, never inside a formatted error chain. The deadline
/// covers the whole grammers sign-in operation, including its password-info RPC.
async fn code_sign_in_with_timeout<T>(
    secs: u64,
    sign_in: impl Future<Output = Result<T, SignInError>>,
) -> Result<Option<PasswordToken>, Error> {
    with_timeout("sign_in", secs, async {
        match sign_in.await {
            Ok(_) => Ok(None),
            Err(SignInError::PasswordRequired(token)) => Ok(Some(token)),
            Err(SignInError::InvalidCode) => {
                Err(Error::Auth("Sign in failed: invalid code".into()))
            }
            Err(SignInError::SignUpRequired) => Err(Error::Auth(
                "Sign in failed: sign up with official client required".into(),
            )),
            Err(_) => Err(Error::Auth("Sign in failed".into())),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn sign_in_rpc_uses_resolve_timeout() {
        use std::future::pending;
        let start = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            code_sign_in_with_timeout::<()>(2, pending()),
        )
        .await;
        let result = result.expect("sign-in must have its own deadline");
        assert!(
            matches!(result, Err(Error::Timeout { ref operation, secs: 2 }) if operation == "sign_in")
        );
        assert_eq!(start.elapsed(), std::time::Duration::from_secs(2));
    }

    #[tokio::test]
    async fn sign_in_preserves_2fa_challenge_and_redacts_failures() {
        use std::future::ready;
        let challenge = code_sign_in_with_timeout::<()>(
            2,
            ready(Err(SignInError::PasswordRequired(password_token()))),
        )
        .await
        .expect("2FA is an outcome")
        .expect("challenge preserved");
        assert_eq!(challenge.hint(), Some("test hint"));
        assert!(
            code_sign_in_with_timeout(2, ready(Ok(())))
                .await
                .expect("success")
                .is_none()
        );
        let error = code_sign_in_with_timeout::<()>(
            2,
            ready(Err(SignInError::InvalidPassword(password_token()))),
        )
        .await
        .expect_err("failure");
        let upstream = grammers_mtsender::InvocationError::Rpc(grammers_mtsender::RpcError {
            code: 400,
            name: "FAKE_AUTH_SECRET_94".into(),
            value: None,
            caused_by: None,
        });
        let upstream_error =
            code_sign_in_with_timeout::<()>(2, ready(Err(SignInError::Other(upstream))))
                .await
                .expect_err("upstream failure");
        for diagnostic in [upstream_error.to_string(), format!("{upstream_error:?}")] {
            assert!(!diagnostic.contains("FAKE_AUTH_SECRET_94"));
        }
        for diagnostic in [error.to_string(), format!("{error:?}")] {
            assert!(!diagnostic.contains("test hint"));
        }
    }

    fn password_token() -> grammers_client::client::PasswordToken {
        grammers_client::client::PasswordToken::new(grammers_client::tl::types::account::Password {
            has_recovery: false, has_secure_values: false, has_password: true,
            current_algo: Some(grammers_client::tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow { salt1: vec![], salt2: vec![], g: 2, p: vec![1] }.into()),
            srp_b: Some(vec![]), srp_id: Some(1), hint: Some("test hint".into()),
            email_unconfirmed_pattern: None,
            new_algo: grammers_client::tl::enums::PasswordKdfAlgo::Unknown,
            new_secure_algo: grammers_client::tl::enums::SecurePasswordKdfAlgo::Unknown,
            secure_random: vec![], pending_reset_date: None, login_email_pattern: None,
        })
    }
    #[tokio::test(start_paused = true)]
    async fn password_rpc_uses_resolve_timeout() {
        let (client, _runner, _dir) =
            crate::telegram::client::client_fixture::stalled_client().await;
        let start = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client.check_password(password_token(), "fake-password"),
        )
        .await;
        assert!(
            matches!(result, Ok(Err(Error::Timeout { ref operation, secs: 2 })) if operation == "check_password"),
            "stalled password RPC must report its own timeout"
        );
        assert_eq!(start.elapsed(), std::time::Duration::from_secs(2));
    }
    #[tokio::test(start_paused = true)]
    async fn login_code_rpc_uses_resolve_timeout() {
        let (client, _runner, _dir) =
            crate::telegram::client::client_fixture::stalled_client().await;
        let start = tokio::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client.request_login_code("+15550000000", "fake-hash"),
        )
        .await;
        assert!(
            matches!(result, Ok(Err(Error::Timeout { ref operation, secs: 2 })) if operation == "request_login_code"),
            "stalled login RPC must report its own timeout"
        );
        assert_eq!(start.elapsed(), std::time::Duration::from_secs(2));
    }
}
