#![warn(missing_docs)]
//! A very simple implementation of a mutex using an `AtomicBool` to synchronize
//! concurrent accesses.

use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};

/// A mutex implemented using an `AtomicBool`.
pub struct Mutex<T: ?Sized> {
    is_locked: AtomicBool,
    data: UnsafeCell<T>,
}

/// A `MutexGuard` is a wrapper struct returned whenever we try to lock the
/// mutex. The reason for using this is so that we can implement `Drop` on the
/// `MutexGuard` and unlock the mutex in the drop implmentation.
pub struct MutexGuard<'a, T: ?Sized + 'a> {
    lock: &'a Mutex<T>,
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.is_locked
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .expect("Mutex should have been locked when the MutexGuard was dropped; but it was already unlocked");
    }
}

unsafe impl<T> Send for Mutex<T> where T: Send {}
unsafe impl<T> Sync for Mutex<T> where T: Send {}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Deref for Mutex<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data.get() }
    }
}

impl<T> DerefMut for Mutex<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.data.get() }
    }
}

impl<T> Mutex<T> {
    /// Create a new mutex wrapping the `data`.
    pub fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            is_locked: AtomicBool::new(false),
        }
    }

    /// Attempt to lock the mutex. This is a spin lock: if the mutex is already
    /// locked then this function will block until we have successfully acquired
    /// the lock.
    pub fn lock(&self) -> Result<MutexGuard<T>, ()> {
        loop {
            match self.try_lock() {
                Err(()) => {
                    std::hint::spin_loop();
                }
                Ok(guard) => return Ok(guard),
            }
        }
    }

    /// Attempt to lock the mutex. Immediately returns an `Err` if the mutex is
    /// already locked.
    pub fn try_lock(&self) -> Result<MutexGuard<T>, ()> {
        if let Ok(_) =
            self.is_locked
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(MutexGuard { lock: self })
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn prevents_access_when_locked() {
        let mutex: Arc<Mutex<u8>> = Arc::new(Mutex::new(0));
        let clone = Arc::clone(&mutex);

        // Immediately lock the mutex for the rest of this test.
        let lock = mutex.try_lock();
        assert!(lock.is_ok());

        thread::spawn(move || {
            let lock = clone.try_lock();
            assert!(lock.is_err());
        })
        .join()
        .expect("thread::spawn failed");
    }

    #[test]
    fn spin_lock_spins() {
        let mutex: Arc<Mutex<u8>> = Arc::new(Mutex::new(0));
        let clone = Arc::clone(&mutex);

        thread::spawn(move || {
            let lock = clone.lock();

            // No reason we can't immediately acquire this lock. The other
            // thread is waiting for a while.
            assert!(lock.is_ok());
            thread::sleep(std::time::Duration::from_millis(2_000));
            // `MutexGuard` will only be dropped after 2 seconds.
        });

        // Wait 100ms to make sure the other thread has acquired the lock.
        thread::sleep(std::time::Duration::from_millis(100));

        // At this point, the other thread has the lock, so we should get an
        // error if we try to acquire it.
        assert!(mutex.try_lock().is_err());

        // Time how long it takes to acquire the lock.
        let start = std::time::Instant::now();
        let lock = mutex.lock();
        let elapsed = start.elapsed().as_millis();

        // It should have taken c. 2 seconds to acquire the lock.
        assert!(elapsed > 1_800);
        // The lock should actually have been acquired.
        assert!(lock.is_ok());
    }
}
