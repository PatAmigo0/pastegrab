//! Stopping a running step from another thread.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

type Hook = Box<dyn FnOnce() + Send>;

/// A flag a step checks between chunks, plus kill hooks the step registers while it holds a
/// child process. Cancelling sets the flag and runs the hooks once.
#[derive(Default)]
pub struct Cancel {
    cancelled: AtomicBool,
    next_hook: AtomicU64,
    hooks: Mutex<Vec<(u64, Hook)>>,
}

/// Names a registered hook so the step can remove it once the work it stops is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct HookId(u64);

impl Cancel {
    /// Sets the flag and runs every registered hook.
    pub fn cancel(&self) {
        let hooks = {
            let mut hooks = self.hooks.lock().unwrap_or_else(PoisonError::into_inner);
            self.cancelled.store(true, Ordering::Release);
            std::mem::take(&mut *hooks)
        };
        for (_, hook) in hooks {
            hook();
        }
    }

    /// Whether `cancel` ran.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Runs `hook` on cancel, or right away when the cancel already happened.
    pub fn on_cancel(&self, hook: impl FnOnce() + Send + 'static) -> HookId {
        let id = self.next_hook.fetch_add(1, Ordering::Relaxed);
        let mut hooks = self.hooks.lock().unwrap_or_else(PoisonError::into_inner);
        if self.is_cancelled() {
            drop(hooks);
            hook();
        } else {
            hooks.push((id, Box::new(hook)));
        }
        HookId(id)
    }

    /// Forgets a hook whose work ended, so a cancel can't hit a process id that was reused.
    pub fn remove(&self, id: HookId) {
        let mut hooks = self.hooks.lock().unwrap_or_else(PoisonError::into_inner);
        hooks.retain(|(hook, _)| *hook != id.0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::Cancel;

    #[test]
    fn hooks_run_once_and_late_hooks_run_at_once() {
        let cancel = Cancel::default();
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        let _ = cancel.on_cancel(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        let removed = Arc::clone(&runs);
        let gone = cancel.on_cancel(move || {
            removed.fetch_add(100, Ordering::Relaxed);
        });
        cancel.remove(gone);
        cancel.cancel();
        cancel.cancel();
        let late = Arc::clone(&runs);
        let _ = cancel.on_cancel(move || {
            late.fetch_add(10, Ordering::Relaxed);
        });
        assert!(cancel.is_cancelled());
        assert_eq!(runs.load(Ordering::Relaxed), 11);
    }
}
