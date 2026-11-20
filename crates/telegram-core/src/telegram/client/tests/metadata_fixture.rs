//! Provider responses for offline hosted metadata route tests using a stalled sender.
use super::TelegramClient;
use super::raw_pager::HistoryFetch;
use crate::telegram::types::TranscriptionOutcome;
use grammers_client::client::DialogIter;
use grammers_client::message::Message;
use grammers_client::peer::Peer;
use grammers_client::{InvocationError, tl};

/// Scripted Telegram observations supplied to a stalled account.
#[derive(Default)]
pub struct MetadataFixture {
    pub dialogs: Vec<tl::enums::Chat>,
    pub history: Option<tl::enums::messages::Messages>,
    pub full_info: Option<(String, u64)>,
    pub last_message_date: Option<crate::UtcTimestamp>,
    pub transcription: Option<TranscriptionOutcome>,
    pub discovery: Option<tl::enums::contacts::Found>,
}

impl TelegramClient {
    /// Install fixture observations before seeding the account registry.
    pub fn set_metadata_fixture(&mut self, data: MetadataFixture) {
        self.metadata_fixture = Some(data);
    }

    /// Record a Premium observation in the account cache used by transcription.
    pub async fn set_fixture_premium(&self, premium: bool) {
        let mut status = self.account_status.write().await;
        status.authorized = true;
        status.premium = Some(premium);
    }

    pub(super) fn fixture_dialogs(&self, live: DialogIter) -> Dialogs {
        match &self.metadata_fixture {
            Some(data) => Dialogs::Fixture(
                data.dialogs
                    .iter()
                    .map(|chat| Peer::from_raw(&self.client, chat.clone()))
                    .collect::<Vec<_>>()
                    .into_iter(),
            ),
            None => Dialogs::Live(Box::new(live)),
        }
    }
}

pub(super) enum Dialogs {
    Fixture(std::vec::IntoIter<Peer>),
    Live(Box<DialogIter>),
}

pub(super) struct Observation {
    peer: Peer,
    pub(super) last_message: Option<Message>,
}

impl Observation {
    pub(super) fn peer(&self) -> &Peer {
        &self.peer
    }
}

impl Dialogs {
    pub(super) async fn next(&mut self) -> Result<Option<Observation>, InvocationError> {
        match self {
            Self::Fixture(rows) => Ok(rows.next().map(|peer| Observation {
                peer,
                last_message: None,
            })),
            Self::Live(live) => Ok(live.next().await?.map(|row| Observation {
                peer: row.peer,
                last_message: row.last_message,
            })),
        }
    }
}

impl HistoryFetch for &TelegramClient {
    async fn fetch(
        &mut self,
        request: &tl::functions::messages::GetHistory,
    ) -> Result<tl::enums::messages::Messages, InvocationError> {
        if let Some(page) = self
            .metadata_fixture
            .as_ref()
            .and_then(|fixture| fixture.history.as_ref())
        {
            return Ok(page.clone());
        }
        self.client.invoke(request).await
    }
}
