use super::{DeadlineIo, Listener};
use axum::Router;
use axum::body::{Body, Bytes};
use futures::task::AtomicWaker;
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use telegram_core::UtcTimestamp;
use telegram_core::config::RateLimitConfig;
use telegram_core::error::Error;
use telegram_core::operations::{CancellationToken, OperationContext};
use telegram_core::rate_limiter::RateLimiter;
use telegram_core::streams::{
    ChannelEvent, ChannelHead, ChannelHeadLookup, ChannelPost, InterruptionReason, QueuedEvent,
    StreamBudget, StreamLimits, test_support,
};
use telegram_core::telegram::types::{ChannelId, MessageId};
use tokio::io::{
    AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, DuplexStream, ReadBuf,
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

const POST: &str = "withheld-post";

struct EmptyHead;

#[async_trait::async_trait]
impl ChannelHeadLookup for EmptyHead {
    async fn channel_heads(&self, channels: &[ChannelId]) -> Result<Vec<ChannelHead>, Error> {
        Ok(channels
            .iter()
            .map(|&channel_id| ChannelHead {
                channel_id,
                head_message_id: None,
            })
            .collect())
    }
}

#[derive(Default)]
struct WriteGate {
    attempt: Notify,
    released: AtomicBool,
    writer: AtomicWaker,
}

impl WriteGate {
    fn block(&self, cx: &mut Context<'_>, contains_post: bool) -> bool {
        if !contains_post {
            return false;
        }
        self.writer.register(cx.waker());
        if self.released.load(Ordering::Acquire) {
            return false;
        }
        self.attempt.notify_one();
        true
    }

    fn release(&self) {
        self.released.store(true, Ordering::Release);
        self.writer.wake();
    }
}

struct HeldOutput {
    socket: DuplexStream,
    gate: Arc<WriteGate>,
}

impl AsyncRead for HeldOutput {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_read(cx, buffer)
    }
}

impl AsyncWrite for HeldOutput {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.gate.block(cx, contains_post(buffer)) {
            return Poll::Pending;
        }
        Pin::new(&mut self.socket).poll_write(cx, buffer)
    }

    fn is_write_vectored(&self) -> bool {
        self.socket.is_write_vectored()
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        if self
            .gate
            .block(cx, buffers.iter().any(|buffer| contains_post(buffer)))
        {
            return Poll::Pending;
        }
        Pin::new(&mut self.socket).poll_write_vectored(cx, buffers)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}

fn contains_post(buffer: &[u8]) -> bool {
    buffer
        .windows(POST.len())
        .any(|part| part == POST.as_bytes())
}

struct OneConnection(Option<DeadlineIo<HeldOutput>>);

impl Listener for OneConnection {
    type Io = DeadlineIo<HeldOutput>;
    type Addr = ();

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.0.take() {
            Some(socket) => (socket, ()),
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(())
    }
}

struct GuardedPost {
    event: QueuedEvent,
    _permit: OwnedSemaphorePermit,
}

impl AsRef<[u8]> for GuardedPost {
    fn as_ref(&self) -> &[u8] {
        self.event.json().as_bytes()
    }
}

#[tokio::test]
async fn http1_holds_buffered_post_authorization_until_socket_write() {
    let budget = Arc::new(StreamBudget::new(8192));
    let (streams, input) = test_support::account_streams(
        Arc::new(EmptyHead),
        Arc::new(RateLimiter::new(&RateLimitConfig::default())),
        StreamLimits {
            max_queue_items: 1,
            ..StreamLimits::default()
        },
        Arc::clone(&budget),
    );
    let context = OperationContext::new(
        Instant::now() + Duration::from_secs(5),
        CancellationToken::new(),
    );
    let channel = ChannelId::new(42).expect("channel");
    let mut subscription = streams
        .subscribe_channel_posts(&[channel], &context)
        .await
        .expect("subscription");
    assert!(matches!(
        subscription.next().await,
        Some(ChannelEvent::Ready(_))
    ));
    let post = |id, text: &str| ChannelPost {
        channel_id: channel,
        message_id: MessageId::new(id).expect("message ID"),
        published_at: UtcTimestamp::from_unix_seconds(1_700_000_000).expect("date"),
        text: text.to_owned(),
        media: None,
        album_id: None,
    };
    input.publish(post(104, POST));
    let permit = Arc::new(Semaphore::new(1));
    let guarded = Bytes::from_owner(GuardedPost {
        event: subscription.next_queued().await.expect("guarded post"),
        _permit: Arc::clone(&permit)
            .try_acquire_owned()
            .expect("output permit"),
    });
    let queued = budget.used_bytes();
    let frame = Arc::new(Mutex::new(Some(guarded)));
    let app = Router::new().route(
        "/",
        axum::routing::get(move || async move {
            Body::from(
                frame
                    .lock()
                    .expect("frame lock")
                    .take()
                    .expect("one request"),
            )
        }),
    );

    let (socket, mut peer) = tokio::io::duplex(8192);
    assert!(socket.is_write_vectored());
    let gate = Arc::new(WriteGate::default());
    let socket = DeadlineIo::new(
        HeldOutput {
            socket,
            gate: Arc::clone(&gate),
        },
        Duration::from_secs(10),
    );
    let membership = subscription.membership_lost();
    assert!(
        socket
            .authorization
            .bind(
                Instant::now() + Duration::from_secs(10),
                membership.clone(),
                CancellationToken::new(),
            )
            .is_ok()
    );
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(OneConnection(Some(socket)), app)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await
            .expect("HTTP server");
    });
    peer.write_all(b"GET / HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n")
        .await
        .expect("HTTP request");
    tokio::time::timeout(Duration::from_secs(2), gate.attempt.notified())
        .await
        .expect("HTTP/1 offers the buffered frame to the socket");

    assert_eq!(
        budget.used_bytes(),
        queued,
        "buffer retains the queued event"
    );
    assert_eq!(
        permit.available_permits(),
        0,
        "buffer holds the output lease"
    );
    input.publish(post(105, "overflow"));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), subscription.next())
            .await
            .expect("queue overflow ends the subscription"),
        Some(ChannelEvent::Interrupted(InterruptionReason::SlowConsumer))
    );
    drop(subscription);
    input.membership_lost(channel);
    assert!(membership.is_cancelled());
    gate.release();

    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), peer.read_to_end(&mut response))
        .await
        .expect("revocation closes the connection")
        .expect("HTTP bytes");
    assert!(!contains_post(&response));
    assert_eq!(budget.used_bytes(), 0);
    assert_eq!(permit.available_permits(), 1);
    shutdown.cancel();
    server.await.expect("HTTP server task");
}
