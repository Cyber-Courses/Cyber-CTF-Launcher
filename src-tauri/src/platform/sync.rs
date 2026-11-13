//! Locks by key: one async lock per lab, per run folder or per run key, handed out by the key
//! and shared by every holder of it, so two operations on one lab wait on each other and two
//! labs never do. Written four times before (the in-flight registry, the regeneration lock, the
//! status read's turn, the lab file locks): one type, one place to evict.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::Mutex as AsyncMutex;

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

/// The locks for keys of type `K`, each guarding a `T` (`()` for a plain lock).
#[derive(Debug)]
pub struct KeyedLocks<K, T = ()> {
    locks: Mutex<BTreeMap<K, Arc<AsyncMutex<T>>>>,
}

impl<K: Ord, T> KeyedLocks<K, T> {
    /// No locks yet; `const`, so a module can keep one in a `static`.
    pub const fn new() -> KeyedLocks<K, T> {
        KeyedLocks { locks: Mutex::new(BTreeMap::new()) }
    }

    /// The lock for `key`, the same one for every caller asking for that key: made on the first
    /// ask (with `T::default()`), then shared. The registry's own lock is held only for the
    /// lookup: a poisoned registry (a panic while holding it) still hands the map back, as the
    /// per-key locks inside are what guard the real work, and losing the registry would wedge
    /// every key.
    pub fn get<Q>(&self, key: &Q) -> Arc<AsyncMutex<T>>
    where
        K: std::borrow::Borrow<Q> + for<'a> From<&'a Q>,
        Q: Ord + ?Sized,
        T: Default,
    {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(lock) = locks.get(key) {
            return Arc::clone(lock);
        }
        let lock = Arc::new(AsyncMutex::new(T::default()));
        locks.insert(K::from(key), Arc::clone(&lock));
        lock
    }

    /// How many keys have a lock.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.locks.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

impl<K: Ord, T> Default for KeyedLocks<K, T> {
    fn default() -> Self {
        KeyedLocks::new()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::KeyedLocks;

    #[tokio::test]
    async fn one_lock_per_key_shared_by_its_askers() {
        let locks: KeyedLocks<String> = KeyedLocks::new();
        let a = locks.get("lab-a");
        let a_again = locks.get("lab-a");
        let b = locks.get("lab-b");
        assert!(Arc::ptr_eq(&a, &a_again), "the same key, the same lock");
        assert!(!Arc::ptr_eq(&a, &b), "another key, another lock");
        assert_eq!(locks.len(), 2);
        let held = a.lock().await;
        assert!(a_again.try_lock().is_err(), "held through one handle, held through the other");
        assert!(b.try_lock().is_ok(), "the other key's lock is free");
        drop(held);
    }

    #[tokio::test]
    async fn a_path_key_is_asked_for_by_its_borrowed_form() {
        static LOCKS: KeyedLocks<PathBuf, Option<u64>> = KeyedLocks::new();
        let lock = LOCKS.get(Path::new("/labs/one"));
        *lock.lock().await = Some(7);
        assert_eq!(*LOCKS.get(Path::new("/labs/one")).lock().await, Some(7), "the guarded value is the key's");
    }

    /// Many tasks on many threads, each taking its key's lock and bumping a counter under it: the
    /// counters end exact, so no two tasks held one key's lock at once (Clean Code ch.13: run
    /// with more threads than processors).
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn the_locks_exclude_across_threads() {
        let locks: Arc<KeyedLocks<String>> = Arc::new(KeyedLocks::new());
        let counters: Arc<Vec<AtomicUsize>> = Arc::new((0..4).map(|_| AtomicUsize::new(0)).collect());
        let mut tasks = Vec::new();
        for i in 0..64 {
            let (locks, counters) = (Arc::clone(&locks), Arc::clone(&counters));
            tasks.push(tokio::spawn(async move {
                let key = format!("key-{}", i % 4);
                let lock = locks.get(key.as_str());
                let _held = lock.lock().await;
                let counter = &counters[i % 4];
                let seen = counter.load(Ordering::SeqCst);
                tokio::task::yield_now().await;
                counter.store(seen + 1, Ordering::SeqCst);
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert!(counters.iter().all(|c| c.load(Ordering::SeqCst) == 16), "every key's lock excluded its takers");
        assert_eq!(locks.len(), 4);
    }
}
