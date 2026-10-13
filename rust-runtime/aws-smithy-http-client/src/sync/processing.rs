/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Thread-local exclusion of synchronous waits inside pool callbacks.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

#[cfg(all(test, smithy_http_client_loom))]
loom::thread_local! {
    static DEPTH: Cell<(usize, usize)> = Cell::new((0, 0));
}
#[cfg(not(all(test, smithy_http_client_loom)))]
std::thread_local! {
    static DEPTH: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// Local cleanup has not yet finished. This guard must stay on its thread.
pub(crate) struct LocalCompletion(PhantomData<Rc<()>>);

impl LocalCompletion {
    pub(crate) fn enter() -> Self {
        DEPTH.with(|depth| {
            let (local, processing) = depth.get();
            depth.set((local + 1, processing));
        });
        Self(PhantomData)
    }

    /// Close this frame and report whether a synchronous nomination is safe.
    pub(crate) fn finish(self) -> bool {
        let ready = DEPTH.with(|depth| depth.get() == (1, 0));
        drop(self);
        ready
    }
}

impl Drop for LocalCompletion {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            let (local, processing) = depth.get();
            depth.set((local - 1, processing));
        });
    }
}

/// An admission processor is active on this thread, including across origins.
///
/// A callback must not nominate and wait for a holder which can itself be
/// waiting for this callback. Tracking only same-origin recursion is insufficient.
pub(crate) struct ProcessingScope(PhantomData<Rc<()>>);

impl ProcessingScope {
    pub(crate) fn enter() -> Self {
        DEPTH.with(|depth| {
            let (local, processing) = depth.get();
            depth.set((local, processing + 1));
        });
        Self(PhantomData)
    }
}

impl Drop for ProcessingScope {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            let (local, processing) = depth.get();
            depth.set((local, processing - 1));
        });
    }
}

/// Wait without holding a coordination lock. Loom must yield to its holder.
pub(crate) fn handoff_backoff(attempt: usize) {
    #[cfg(all(test, smithy_http_client_loom))]
    {
        let _ = attempt;
        loom::thread::yield_now();
    }
    #[cfg(not(all(test, smithy_http_client_loom)))]
    if attempt < 16 {
        std::hint::spin_loop();
    } else {
        std::thread::yield_now();
    }
}
