//! Small concurrency helpers: a single-flight flag ([`Busy`]) and a task owned by whoever keeps
//! it ([`OwnedTask`]).

/// A single-flight flag taken for as long as the guard lives: a scan or a download that runs
/// once at a time, whoever asks meanwhile told so. Released on drop, so a cancelled flight frees
/// the next one. Written twice before (`ResetOnDrop`, the downloads' `Busy`); one type.
pub struct Busy(&'static std::sync::atomic::AtomicBool);

impl Busy {
    /// Takes `flag` when it is free; `None` while another flight holds it.
    pub fn try_take(flag: &'static std::sync::atomic::AtomicBool) -> Option<Busy> {
        use std::sync::atomic::Ordering;
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| Busy(flag))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// A background task owned by whoever keeps this: aborted when it is dropped, so the task lives
/// exactly as long as its owner (a loop beside another, ended and cancelled with it), never
/// detached for the life of the process.
#[derive(Debug)]
pub struct OwnedTask(tokio::task::JoinHandle<()>);

impl OwnedTask {
    /// Spawns `work` on the current runtime, owned by the value returned.
    pub fn spawn(work: impl std::future::Future<Output = ()> + Send + 'static) -> OwnedTask {
        OwnedTask(tokio::spawn(work))
    }
}

impl Drop for OwnedTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    #[tokio::test(start_paused = true)]
    async fn an_owned_task_ends_with_its_owner() {
        let alive = Arc::new(());
        let task = {
            let alive = Arc::clone(&alive);
            super::OwnedTask::spawn(async move {
                std::future::pending::<()>().await;
                drop(alive);
            })
        };
        tokio::task::yield_now().await;
        assert_eq!(Arc::strong_count(&alive), 2, "running while owned");
        drop(task);
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(Arc::strong_count(&alive), 1, "aborted, what it held dropped, once its owner let go");
    }
}
