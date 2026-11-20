//! Blocking image jobs remain owned by their worker even if a request is cancelled.

pub(crate) const IMAGE_PROCESSING_SLOTS: usize = 2;
static IMAGE_JOBS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(IMAGE_PROCESSING_SLOTS);

/// Execute image CPU work off the async executor. The caller's future can be
/// dropped, but a started worker still runs to completion.
pub(crate) async fn image_job<F, T>(job: F) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // Private static semaphore has no close path. The permit belongs to the
    // blocking closure, not to the cancellable async waiter.
    let permit = IMAGE_JOBS
        .acquire()
        .await
        .expect("private image semaphore is never closed");
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        job()
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn cancelled_awaiters_do_not_release_started_image_capacity() {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (started, mut observed) = mpsc::unbounded_channel();
        let mut tasks = Vec::new();
        for _ in 0..3 {
            let release = Arc::clone(&release);
            let started = started.clone();
            tasks.push(tokio::spawn(image_job(move || {
                started.send(()).expect("started");
                let (lock, cv) = &*release;
                let mut ready = lock.lock().expect("fixture lock");
                while !*ready {
                    ready = cv.wait(ready).expect("fixture wait");
                }
            })));
        }
        observed.recv().await.expect("first worker");
        observed.recv().await.expect("second worker");
        for task in &tasks {
            task.abort();
        }
        let third =
            tokio::time::timeout(std::time::Duration::from_millis(50), observed.recv()).await;
        let (lock, cv) = &*release;
        *lock.lock().expect("fixture lock") = true;
        cv.notify_all();
        for task in tasks {
            let _ = task.await;
        }
        assert!(
            third.is_err(),
            "a third CPU worker started while two cancelled jobs still owned capacity"
        );
    }
}
