//! HTTP connection writer with progress-based stall detection.

use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use futures::task::AtomicWaker;
use std::future::Future;
use std::io::{self, IoSlice};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::Duration;
use telegram_core::operations::CancellationToken;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Sleep;

#[derive(Clone, Default)]
pub(super) struct WriteAuthorization(Arc<WriteState>);

#[derive(Default)]
struct WriteState {
    access: Mutex<WriterAccess>,
    writer: AtomicWaker,
}

#[derive(Default)]
struct WriterAccess {
    stopped: bool,
    stream: Option<StreamAccess>,
}

struct StreamAccess {
    expires: tokio::time::Instant,
    membership_lost: telegram_core::operations::CancellationToken,
    generation: telegram_core::operations::CancellationToken,
}

impl StreamAccess {
    fn invalid(&self) -> bool {
        tokio::time::Instant::now() >= self.expires
            || self.membership_lost.is_cancelled()
            || self.generation.is_cancelled()
    }
}

impl WriteAuthorization {
    pub(super) fn revoke(&self) {
        if let Ok(mut access) = self.0.access.lock() {
            access.stopped = true;
        }
        self.0.writer.wake();
    }

    pub(super) fn bind(
        &self,
        expires: tokio::time::Instant,
        membership_lost: telegram_core::operations::CancellationToken,
        generation: telegram_core::operations::CancellationToken,
    ) -> Result<(), super::ApiError> {
        let stream = StreamAccess {
            expires,
            membership_lost,
            generation,
        };
        let mut access = self
            .0
            .access
            .lock()
            .map_err(|_| super::ApiError::new(crate::contracts::ErrorCode::Unavailable))?;
        if access.stopped || access.stream.is_some() || stream.invalid() {
            return Err(super::ApiError::new(
                crate::contracts::ErrorCode::UnavailableResource,
            ));
        }
        access.stream = Some(stream);
        Ok(())
    }

    fn write<U>(
        &self,
        cx: &mut Context<'_>,
        submit: impl FnOnce(&mut Context<'_>) -> Poll<io::Result<U>>,
    ) -> Poll<io::Result<U>> {
        self.0.writer.register(cx.waker());
        let access = self.0.access.lock().map_err(|_| closed())?;
        if access.stopped || access.stream.as_ref().is_some_and(StreamAccess::invalid) {
            return Poll::Ready(Err(closed()));
        }
        submit(cx)
    }
}

fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "stream authorization ended",
    )
}

#[derive(Default)]
pub(super) struct ActiveWriters(Mutex<Vec<RegisteredWriter>>);

struct RegisteredWriter {
    generation: super::AccountGeneration,
    client: String,
    writer: Weak<WriteState>,
}

impl ActiveWriters {
    pub(super) fn register(
        &self,
        access: &super::authorization::ReadAccess,
        writer: &WriteAuthorization,
    ) -> Result<(), super::ApiError> {
        let mut active = self
            .0
            .lock()
            .map_err(|_| super::ApiError::new(crate::contracts::ErrorCode::Unavailable))?;
        active.retain(|entry| entry.writer.strong_count() > 0);
        active.push(RegisteredWriter {
            generation: access.generation.clone(),
            client: access.identity.client_id().to_owned(),
            writer: Arc::downgrade(&writer.0),
        });
        Ok(())
    }

    pub(super) fn revoke(&self, generation: &super::AccountGeneration, client: Option<&str>) {
        if let Ok(mut active) = self.0.lock() {
            active.retain(|entry| {
                let Some(writer) = entry.writer.upgrade() else {
                    return false;
                };
                if &entry.generation == generation
                    && client.is_none_or(|client| client == entry.client)
                {
                    WriteAuthorization(writer).revoke();
                    return false;
                }
                true
            });
        }
    }
}

pub(super) struct DeadlineListener {
    listener: TcpListener,
    write_timeout: Duration,
    hard_stop: CancellationToken,
}

impl DeadlineListener {
    pub(super) fn new(
        listener: TcpListener,
        write_timeout: Duration,
        hard_stop: CancellationToken,
    ) -> Self {
        Self {
            listener,
            write_timeout,
            hard_stop,
        }
    }
}

impl Listener for DeadlineListener {
    type Io = DeadlineIo<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (socket, addr) = Listener::accept(&mut self.listener).await;
        let mut io = DeadlineIo::new(socket, self.write_timeout);
        io.hard_stop = Some(Box::pin(self.hard_stop.clone().cancelled_owned()));
        (io, addr)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

impl Connected<IncomingStream<'_, DeadlineListener>> for WriteAuthorization {
    fn connect_info(stream: IncomingStream<'_, DeadlineListener>) -> Self {
        stream.io().authorization.clone()
    }
}

pub(super) struct DeadlineIo<T> {
    inner: T,
    timeout: Duration,
    timer: Option<Pin<Box<Sleep>>>,
    authorization: WriteAuthorization,
    hard_stop: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl<T> DeadlineIo<T> {
    fn new(inner: T, timeout: Duration) -> Self {
        Self {
            inner,
            timeout,
            timer: None,
            authorization: WriteAuthorization::default(),
            hard_stop: None,
        }
    }

    fn check_drain(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        if self
            .hard_stop
            .as_mut()
            .is_some_and(|stop| stop.as_mut().poll(cx).is_ready())
        {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "service drain ended",
            ));
        }
        Ok(())
    }

    fn track<U>(
        &mut self,
        cx: &mut Context<'_>,
        progress: Poll<io::Result<U>>,
    ) -> Poll<io::Result<U>> {
        match progress {
            Poll::Ready(result) => {
                self.timer = None;
                Poll::Ready(result)
            }
            Poll::Pending => {
                let timer = self
                    .timer
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(self.timeout)));
                if timer.as_mut().poll(cx).is_ready() {
                    Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "hosted response write stalled",
                    )))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for DeadlineIo<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.check_drain(cx)?;
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for DeadlineIo<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.check_drain(cx)?;
        let this = self.as_mut().get_mut();
        let progress = this
            .authorization
            .write(cx, |cx| Pin::new(&mut this.inner).poll_write(cx, buf));
        self.track(cx, progress)
    }

    fn is_write_vectored(&self) -> bool {
        // Hyper queues owned body buffers for vectored writes. Its fallback
        // copies frames and can drop their reservations before socket progress.
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.check_drain(cx)?;
        let this = self.as_mut().get_mut();
        let progress = this.authorization.write(cx, |cx| {
            Pin::new(&mut this.inner).poll_write_vectored(cx, buffers)
        });
        self.track(cx, progress)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.check_drain(cx)?;
        let this = self.as_mut().get_mut();
        let progress = this
            .authorization
            .write(cx, |cx| Pin::new(&mut this.inner).poll_flush(cx));
        self.track(cx, progress)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.check_drain(cx)?;
        let progress = Pin::new(&mut self.inner).poll_shutdown(cx);
        self.track(cx, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt as _;

    #[tokio::test(start_paused = true)]
    async fn stalled_write_times_out_and_revocation_wakes_pending_transport() {
        let (writer, _reader) = tokio::io::duplex(1);
        let mut io = DeadlineIo::new(writer, Duration::from_secs(10));
        let start = tokio::time::Instant::now();
        let error = io.write_all(b"blocked").await.expect_err("bounded write");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(start.elapsed(), Duration::from_secs(10));

        let (writer, _reader) = tokio::io::duplex(1);
        let mut io = DeadlineIo::new(writer, Duration::from_secs(10));
        let authorization = io.authorization.clone();
        let revoke = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            authorization.revoke();
        });
        let error = io.write_all(b"blocked").await.expect_err("revoked writer");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        revoke.await.expect("revocation task");
    }
    #[tokio::test(start_paused = true)]
    async fn write_checks_expiry_and_membership_without_an_async_watcher() {
        let (writer, _reader) = tokio::io::duplex(100);
        let mut io = DeadlineIo::new(writer, Duration::from_secs(10));
        let member = CancellationToken::new();
        assert!(
            io.authorization
                .bind(
                    tokio::time::Instant::now() + Duration::from_secs(3),
                    member.clone(),
                    CancellationToken::new()
                )
                .is_ok()
        );
        io.write_all(b"before").await.expect("eligible write");
        member.cancel();
        assert_eq!(
            io.write_all(b"after")
                .await
                .expect_err("membership lost")
                .kind(),
            io::ErrorKind::ConnectionAborted
        );

        let (writer, _reader) = tokio::io::duplex(100);
        let mut io = DeadlineIo::new(writer, Duration::from_secs(10));
        assert!(
            io.authorization
                .bind(
                    tokio::time::Instant::now() + Duration::from_secs(3),
                    CancellationToken::new(),
                    CancellationToken::new()
                )
                .is_ok()
        );
        tokio::time::advance(Duration::from_secs(3)).await;
        assert_eq!(
            io.flush().await.expect_err("expired credential").kind(),
            io::ErrorKind::ConnectionAborted
        );
    }
    #[tokio::test]
    async fn stalled_http_peer_releases_in_flight_response() {
        use axum::body::{Body, Bytes};
        use std::convert::Infallible;
        use tokio::sync::Semaphore;

        let lease = Arc::new(Semaphore::new(1));
        let permit = Arc::clone(&lease)
            .acquire_owned()
            .await
            .expect("response lease");
        let published = Arc::new(tokio::sync::Notify::new());
        let handler_notified = Arc::clone(&published);
        let saved = Arc::new(Mutex::new(Some(permit)));
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(move || async move {
                let permit = saved
                    .lock()
                    .expect("lease lock")
                    .take()
                    .expect("one request");
                handler_notified.notify_one();
                let stream = futures::stream::unfold(permit, |permit| async {
                    let chunk = Bytes::from_static(&[b'x'; 64 * 1024]);
                    Some((Ok::<_, Infallible>(chunk), permit))
                });
                Body::from_stream(stream)
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("server");
        let address = listener.local_addr().expect("address");
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let server = tokio::spawn(async move {
            super::super::serve(listener, app, Duration::from_millis(200), async move {
                shutdown.cancelled().await;
            })
            .await
            .expect("serve");
        });
        let peer = tokio::net::TcpSocket::new_v4().expect("peer socket");
        peer.set_recv_buffer_size(4_096)
            .expect("bounded receive window");
        let mut peer = peer.connect(address).await.expect("connect");
        peer.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("request");
        tokio::time::timeout(Duration::from_secs(2), published.notified())
            .await
            .expect("response started");
        let _released = tokio::time::timeout(Duration::from_secs(3), lease.acquire_owned())
            .await
            .expect("response writer disconnected")
            .expect("response lease released");
        drop(peer);
        stop.cancel();
        server.await.expect("bounded server stop");
    }
}

#[cfg(test)]
#[path = "event_writer/http_tests.rs"]
mod http_tests;
