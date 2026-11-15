//! Tests that spawn a child process (`hdiutil`, `pmset`) and tests that let a port go and bind it
//! again can't run at the same moment (#161). On macOS `posix_spawn` copies every open socket
//! into the child, and std only marks them close-on-exec in a later step, so a child spawned in
//! that gap holds a port that was just let go, for tens of microseconds, and the rebind finds it
//! in use. The service itself never spawns a child, so only tests need this.

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

static LOCK: RwLock<()> = RwLock::new(());

/// Held around spawning a child: any number of spawns at once, none while a port is let go.
pub fn spawning() -> RwLockReadGuard<'static, ()> {
    LOCK.read().unwrap_or_else(|e| e.into_inner())
}

/// Held across letting a port go and binding it again: no child is spawned meanwhile.
pub fn rebinding() -> RwLockWriteGuard<'static, ()> {
    LOCK.write().unwrap_or_else(|e| e.into_inner())
}
