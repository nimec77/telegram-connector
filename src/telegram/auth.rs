use crate::error::Error;
use dialoguer::{Input, Password};
use grammers_client::{Client, SignInError};
use std::fs;
use std::path::Path;

/// Save a Telegram session to a file with secure permissions (0600)
///
/// The session bytes should be obtained from `client.session().save()`.
pub fn save_session(path: &Path, session_bytes: &[u8]) -> Result<(), Error> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::Auth(format!("Failed to create session directory: {}", e)))?;
    }

    // Write to temp file first (atomic write pattern)
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, session_bytes)
        .map_err(|e| Error::Auth(format!("Failed to write session file: {}", e)))?;

    // Set permissions to 0600 (owner read/write only) on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&temp_path, permissions)
            .map_err(|e| Error::Auth(format!("Failed to set session file permissions: {}", e)))?;
    }

    // Atomic rename
    fs::rename(&temp_path, path)
        .map_err(|e| Error::Auth(format!("Failed to finalize session file: {}", e)))?;

    Ok(())
}

/// Load a Telegram session from a file, verifying secure permissions
///
/// Returns the session bytes which can be used with `Client::connect()`.
pub fn load_session(path: &Path) -> Result<Vec<u8>, Error> {
    // Check file exists
    if !path.exists() {
        return Err(Error::Auth(format!(
            "Session file does not exist: {}",
            path.display()
        )));
    }

    // Check permissions on Unix (must be 0600)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path)
            .map_err(|e| Error::Auth(format!("Failed to read session file metadata: {}", e)))?;
        let mode = metadata.permissions().mode() & 0o777;
        if mode != 0o600 {
            return Err(Error::Auth(format!(
                "Session file has insecure permissions: {:o} (expected 0600)",
                mode
            )));
        }
    }

    // Read session bytes
    fs::read(path).map_err(|e| Error::Auth(format!("Failed to read session file: {}", e)))
}

/// Check if a Telegram client session is still valid
pub async fn is_session_valid(client: &Client) -> bool {
    client.is_authorized().await.unwrap_or(false)
}

/// Interactive authentication flow for Telegram
///
/// This prompts the user for:
/// - Authentication code (sent to Telegram app)
/// - 2FA password (if enabled on account)
///
/// The phone number should already be used when requesting the login code.
///
/// Returns Ok(()) if authentication succeeds.
pub async fn authenticate(client: &Client, phone: &str) -> Result<(), Error> {
    // Request login code (grammers requires phone and code settings)
    let token = client
        .request_login_code(phone, "")
        .await
        .map_err(|e| Error::Auth(format!("Failed to request login code: {}", e)))?;

    // Prompt for code
    let code: String = Input::new()
        .with_prompt("Enter the code you received in Telegram")
        .interact_text()
        .map_err(|e| Error::Auth(format!("Failed to read input: {}", e)))?;

    // Sign in with code
    match client.sign_in(&token, &code).await {
        Ok(_) => {
            tracing::info!("Successfully authenticated");
            Ok(())
        }
        Err(SignInError::PasswordRequired(password_token)) => {
            // 2FA is enabled, prompt for password
            let password = Password::new()
                .with_prompt("Enter your 2FA password")
                .interact()
                .map_err(|e| Error::Auth(format!("Failed to read password: {}", e)))?;

            client
                .check_password(password_token, password.trim())
                .await
                .map_err(|e| Error::Auth(format!("2FA authentication failed: {}", e)))?;

            tracing::info!("Successfully authenticated with 2FA");
            Ok(())
        }
        Err(e) => Err(Error::Auth(format!("Sign in failed: {}", e))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn save_session_creates_file() {
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("test.session");
        let session_data = b"test session data";

        let result = save_session(&session_path, session_data);
        assert!(result.is_ok());
        assert!(session_path.exists());
    }

    #[test]
    fn save_session_creates_parent_directory() {
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("subdir").join("test.session");
        let session_data = b"test session data";

        let result = save_session(&session_path, session_data);
        assert!(result.is_ok());
        assert!(session_path.exists());
        assert!(session_path.parent().unwrap().exists());
    }

    #[test]
    #[cfg(unix)]
    fn save_session_sets_correct_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("test.session");
        let session_data = b"test session data";

        save_session(&session_path, session_data).unwrap();

        let metadata = fs::metadata(&session_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn load_session_from_saved_file() {
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("test.session");
        let original_data = b"test session data";

        // Save then load
        save_session(&session_path, original_data).unwrap();
        let loaded_data = load_session(&session_path);

        assert!(loaded_data.is_ok());
        assert_eq!(loaded_data.unwrap(), original_data);
    }

    #[test]
    fn load_session_nonexistent_file_fails() {
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("nonexistent.session");

        let result = load_session(&session_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    #[cfg(unix)]
    fn load_session_rejects_insecure_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("test.session");
        let session_data = b"test session data";

        // Save with correct permissions
        save_session(&session_path, session_data).unwrap();

        // Change to insecure permissions (0644 - world readable)
        let permissions = fs::Permissions::from_mode(0o644);
        fs::set_permissions(&session_path, permissions).unwrap();

        // Load should fail
        let result = load_session(&session_path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("insecure permissions")
        );
    }

    #[test]
    fn save_and_load_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("test.session");
        let original_data = b"test session data with special chars: \x00\x01\xFF";

        // Save
        save_session(&session_path, original_data).unwrap();

        // Load
        let loaded_data = load_session(&session_path).unwrap();

        // Should match exactly
        assert_eq!(original_data, loaded_data.as_slice());
    }

    #[test]
    fn save_overwrites_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let session_path = temp_dir.path().join("test.session");

        // Save first version
        save_session(&session_path, b"version 1").unwrap();

        // Save second version
        save_session(&session_path, b"version 2").unwrap();

        // Load should get second version
        let loaded_data = load_session(&session_path).unwrap();
        assert_eq!(loaded_data, b"version 2");
    }

    // Note: is_session_valid and authenticate tests require a real Telegram client
    // and are tested manually or via integration tests
}
