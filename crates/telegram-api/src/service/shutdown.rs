//! One drain deadline for the HTTP listener and hosted Telegram tasks.

use super::accounts::Hosted;
use super::event_writer::{DeadlineListener, WriteAuthorization};
use axum::Router;
use std::future::IntoFuture as _;
use std::sync::Arc;
use std::time::Duration;
use telegram_core::operations::CancellationToken;
use tokio::net::TcpListener;
use tokio::time::Instant;

pub(super) const DRAIN_LIMIT: Duration = Duration::from_secs(10);

pub(super) async fn serve(
    listener: TcpListener,
    app: Router,
    write_timeout: Duration,
    hosted: Option<Arc<Hosted>>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let stop = CancellationToken::new();
    let listener_stop = stop.clone();
    let hard_stop = CancellationToken::new();
    let listener_task = axum::serve(
        DeadlineListener::new(listener, write_timeout, hard_stop.clone()),
        app.into_make_service_with_connect_info::<WriteAuthorization>(),
    )
    .with_graceful_shutdown(async move { listener_stop.cancelled().await });
    let mut server = std::pin::pin!(listener_task.into_future());
    let early_result = tokio::select! {
        result = &mut server => Some(result),
        () = shutdown => None,
    };
    let deadline = hosted.as_ref().map_or_else(
        || Instant::now() + DRAIN_LIMIT,
        |service| service.begin_draining(),
    );
    stop.cancel();
    let http = async {
        match early_result {
            Some(result) => result,
            None => server.await,
        }
    };
    let accounts = async {
        if let Some(service) = hosted {
            service.wait_drained().await;
        }
    };
    match tokio::time::timeout_at(deadline, async { tokio::join!(http, accounts).0 }).await {
        Ok(result) => result,
        Err(_) => {
            hard_stop.cancel();
            tokio::task::yield_now().await;
            Ok(())
        }
    }
}
