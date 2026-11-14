//! A bounded AsyncRead whose uncancellable filesystem jobs retain snapshot leases.
use super::StagedUpload;
use std::fs::File;
use std::io::{self, Read as _};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

/// Single-use snapshot stream. No arbitrary path constructor is exposed.
pub struct StagedReader {
    file: Option<File>,
    pending: Option<tokio::task::JoinHandle<ReadJob>>,
    bytes: Vec<u8>,
    offset: usize,
    eof: bool,
    owner: Arc<StagedUpload>,
}
impl StagedReader {
    pub(super) fn new(file: File, owner: Arc<StagedUpload>) -> Self {
        Self {
            file: Some(file),
            pending: None,
            bytes: Vec::new(),
            offset: 0,
            eof: false,
            owner,
        }
    }
    fn job(&mut self, size: usize) -> io::Result<ReadJob> {
        if self.owner.deadline().expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Upload read deadline expired",
            ));
        }
        Ok(ReadJob {
            file: self
                .file
                .take()
                .ok_or_else(|| io::Error::other("Upload reader unavailable"))?,
            bytes: vec![0; size.min(524_288)],
            result: Ok(0),
            owner: Arc::clone(&self.owner),
        })
    }
}
struct ReadJob {
    file: File,
    bytes: Vec<u8>,
    result: io::Result<usize>,
    owner: Arc<StagedUpload>,
}
impl ReadJob {
    fn run(mut self) -> Self {
        self.result = if self.owner.deadline().expired() {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Upload read deadline expired",
            ))
        } else {
            self.file
                .read(&mut self.bytes)
                .map_err(|_| io::Error::other("Upload source unavailable"))
        };
        self
    }
}
impl AsyncRead for StagedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if out.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            if self.owner.deadline().expired() {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Upload read deadline expired",
                )));
            }
            if self.offset < self.bytes.len() {
                let n = (self.bytes.len() - self.offset).min(out.remaining());
                out.put_slice(&self.bytes[self.offset..self.offset + n]);
                self.offset += n;
                return Poll::Ready(Ok(()));
            }
            if self.eof {
                return Poll::Ready(Ok(()));
            }
            if self.pending.is_none() {
                let runtime = match tokio::runtime::Handle::try_current() {
                    Ok(r) => r,
                    Err(_) => {
                        return Poll::Ready(Err(io::Error::other("Upload reader unavailable")));
                    }
                };
                let job = match self.job(out.remaining()) {
                    Ok(job) => job,
                    Err(e) => return Poll::Ready(Err(e)),
                };
                self.pending = Some(runtime.spawn_blocking(move || job.run()));
            }
            let Some(job) = self.pending.as_mut() else {
                return Poll::Ready(Err(io::Error::other("Upload reader unavailable")));
            };
            let result = match Pin::new(job).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(r) => r,
            };
            self.pending = None;
            let job = match result {
                Ok(job) => job,
                Err(_) => return Poll::Ready(Err(io::Error::other("Upload reader unavailable"))),
            };
            self.file = Some(job.file);
            self.offset = 0;
            match job.result {
                Ok(n) => {
                    self.bytes = job.bytes;
                    self.bytes.truncate(n);
                    self.eof = n == 0;
                }
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
    }
}

#[cfg(all(test, unix))]
#[path = "tests/reader_tests.rs"]
mod tests;
