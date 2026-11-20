use super::*;
use crate::identity::Audience;
use crate::identity_http::Fixture as Issuer;
use std::net::Ipv4Addr;

struct Provider;

#[async_trait::async_trait]
impl LoginChallenge for Provider {
    async fn code_requires_password(&mut self, code: &str) -> Result<bool, ApiError> {
        assert_eq!(code, "12345");
        Ok(true)
    }

    async fn password(&mut self, password: &str) -> Result<(), ApiError> {
        assert_eq!(password, "test-password");
        Ok(())
    }

    async fn account_id(&self) -> Result<i64, ApiError> {
        Ok(42)
    }

    async fn close(&self) {}
}

async fn login_provider(
    _: &TelegramConfig,
    _: &str,
    _: &str,
) -> Result<Box<dyn LoginChallenge>, ApiError> {
    Ok(Box::new(Provider))
}

#[tokio::test]
async fn challenges_check_owner_and_step_and_expire_or_replace_without_leaking_the_account() {
    let issuer = Issuer::start().await;
    let verifier = issuer.read_verifier();
    let owner = verifier
        .verify(
            &SecretString::from(issuer.read_token("first", false)),
            &Audience::Api,
        )
        .await
        .expect("verified owner");
    let other = verifier
        .verify(
            &SecretString::from(issuer.read_token("other", false)),
            &Audience::Api,
        )
        .await
        .expect("verified other");
    let root = tempfile::tempdir().expect("root");
    let store = Arc::new(
        AccountStore::open(root.path())
            .await
            .expect("account store"),
    );
    let config: TelegramConfig = serde_json::from_value(serde_json::json!({
        "api_id":1, "api_hash":"fixture"
    }))
    .expect("telegram application");
    let linking = Linking::default();
    let start = async |owner: &Principal| {
        linking
            .start_using(
                &store,
                owner,
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                &config,
                SecretString::from("+15550101000"),
                login_provider,
            )
            .await
            .unwrap_or_else(|_| panic!("send code"))
    };
    let owner_id = owner.principal();
    let peer_id = other.principal();
    let original = start(owner_id).await.challenge.expect("first challenge");
    let fresh = start(owner_id)
        .await
        .challenge
        .expect("replacement challenge");
    assert_ne!(original, fresh);
    let code = async |owner: &Principal, id: &str| {
        linking
            .code(&store, owner, id, SecretString::from("12345"))
            .await
    };
    let password = async |owner: &Principal, id: &str| {
        linking
            .password(&store, owner, id, SecretString::from("test-password"))
            .await
    };
    assert_eq!(
        code(owner_id, &original).await.expect_err("replaced").0,
        ErrorCode::UnavailableResource
    );
    assert_eq!(
        code(peer_id, &fresh).await.expect_err("wrong owner").0,
        ErrorCode::UnavailableResource
    );
    assert_eq!(
        password(owner_id, &fresh).await.expect_err("wrong step").0,
        ErrorCode::Conflict
    );
    assert_eq!(
        code(owner_id, &fresh)
            .await
            .unwrap_or_else(|_| panic!("valid code"))
            .step,
        NextStep::Password
    );
    assert_eq!(
        code(owner_id, &fresh).await.expect_err("code used").0,
        ErrorCode::Conflict
    );
    assert_eq!(
        password(owner_id, &fresh)
            .await
            .unwrap_or_else(|_| panic!("valid password"))
            .step,
        NextStep::Linked
    );

    let other = start(peer_id).await.challenge.expect("other challenge");
    assert_eq!(
        code(peer_id, &other)
            .await
            .unwrap_or_else(|_| panic!("valid code"))
            .step,
        NextStep::Password
    );
    assert_eq!(
        password(peer_id, &other)
            .await
            .expect_err("Telegram user already linked")
            .0,
        ErrorCode::Conflict
    );

    let expired = start(peer_id).await.challenge.expect("new other challenge");
    tokio::time::pause();
    tokio::time::advance(ATTEMPT_TTL).await;
    assert_eq!(
        code(peer_id, &expired).await.expect_err("expired").0,
        ErrorCode::UnavailableResource
    );
    linking
        .reap(&store)
        .await
        .unwrap_or_else(|_| panic!("expired challenge cleanup"));
}
