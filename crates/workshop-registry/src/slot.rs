//! The typed proxy slot: one per subsystem, `RwLock<Option<Arc<dyn
//! Trait>>>` under the hood.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock, Weak};

/// One subsystem's proxy slot: empty until the subsystem self-registers.
///
/// Consumers read through [`ProxySlot::get`] and treat `None` as a
/// graceful no-op - an unregistered slot degrades the feature, never
/// fails the caller.
pub struct ProxySlot<T: ?Sized> {
    occupant: Arc<RwLock<Option<Arc<T>>>>,
}

impl<T: ?Sized> ProxySlot<T> {
    /// An empty slot.
    pub(crate) fn new() -> Self {
        Self {
            occupant: Arc::new(RwLock::new(None)),
        }
    }

    /// The registered subsystem, or `None` while the slot is empty.
    #[must_use]
    pub fn get(&self) -> Option<Arc<T>> {
        self.occupant
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Registers `subsystem`, replacing any previous occupant, and
    /// returns the guard keeping the registration alive: dropping the
    /// guard deregisters the subsystem. A replaced occupant's stale
    /// guard deregisters nothing.
    pub fn register(&self, subsystem: Arc<T>) -> Registration<T> {
        *self
            .occupant
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&subsystem));
        Registration {
            slot: Arc::downgrade(&self.occupant),
            subsystem,
        }
    }
}

impl<T: ?Sized> Clone for ProxySlot<T> {
    fn clone(&self) -> Self {
        Self {
            occupant: Arc::clone(&self.occupant),
        }
    }
}

impl<T: ?Sized> fmt::Debug for ProxySlot<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProxySlot")
            .field("registered", &self.get().is_some())
            .finish()
    }
}

/// The registration guard: keeps the subsystem registered while held.
///
/// Dropping the guard deregisters the subsystem, unless the slot has
/// since been re-registered - a stale guard never evicts a newer
/// occupant.
#[must_use = "dropping the guard deregisters the subsystem"]
pub struct Registration<T: ?Sized> {
    slot: Weak<RwLock<Option<Arc<T>>>>,
    subsystem: Arc<T>,
}

impl<T: ?Sized> Drop for Registration<T> {
    fn drop(&mut self) {
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        let mut occupant = slot.write().unwrap_or_else(PoisonError::into_inner);
        if occupant
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &self.subsystem))
        {
            occupant.take();
        }
    }
}

impl<T: ?Sized> fmt::Debug for Registration<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Registration").finish()
    }
}
