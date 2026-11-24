/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{state::MetricsState, RequestMetrics};

/// Extension methods on [`http::Request`] for accessing standard server metrics.
///
/// This trait is sealed and cannot be implemented outside this crate.
pub trait RequestExt: sealed::Sealed {
    /// Registers a callback to be invoked once the request is finished, meaning the response has
    /// been fully sent or sending it has failed.
    ///
    /// The callback receives the standard server metrics collected for the request, or `None` if
    /// standard metrics are disabled or the request never reached the Smithy service. Callbacks are
    /// invoked in the order they were registered.
    ///
    /// This method may be called from a [Layer] or [Plugin] configured via the service's server
    /// SDK, or from a [Layer] applied around the service. A callback registered before the request
    /// reaches the service is invoked once the request finishes, like any other. If the request
    /// never reaches the Smithy service, the callback will be invoked with `None` when the request
    /// is dropped.
    ///
    /// [Layer]: tower::Layer
    /// [Plugin]: crate::plugin::Plugin
    fn on_finish<F>(&mut self, callback: F)
    where
        F: FnOnce(Option<RequestMetrics>) + Send + 'static;
}

impl<B> RequestExt for http::Request<B> {
    fn on_finish<F>(&mut self, callback: F)
    where
        F: FnOnce(Option<RequestMetrics>) + Send + 'static,
    {
        match self.extensions().get::<MetricsState>() {
            Some(state) => state.push_callback(Box::new(callback)),
            // Provisional metrics state, just to store callbacks until MetricsService takes over
            // the request.
            None => {
                let state = MetricsState::uninitialized();
                state.push_callback(Box::new(callback));
                self.extensions_mut().insert(state);
            }
        }
    }
}

mod sealed {
    pub trait Sealed {}
    impl<B> Sealed for http::Request<B> {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::metrics::{state::ServiceIdentity, test_util::TestService};

    use super::*;

    #[test]
    fn registers_callbacks_in_order() {
        let state = MetricsState::new(true, &Default::default(), &ServiceIdentity::of::<TestService>(), 0);
        let mut req = http::Request::new(());
        req.extensions_mut().insert(state.clone());
        req.on_finish(|_| {});
        req.on_finish(|_| {});
        assert_eq!(state.callback_count(), 2);
    }

    #[test]
    fn registers_callbacks_when_disabled() {
        let state = MetricsState::new(false, &Default::default(), &ServiceIdentity::of::<TestService>(), 0);
        let mut req = http::Request::new(());
        req.extensions_mut().insert(state.clone());
        req.on_finish(|_| {});
        assert_eq!(state.callback_count(), 1);
    }

    #[test]
    fn callbacks_of_a_request_that_never_reaches_a_service_receive_none_when_it_is_dropped() {
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut req = http::Request::new(());
        for index in 0..2 {
            let calls = calls.clone();
            req.on_finish(move |metrics| calls.lock().unwrap().push((index, metrics.is_some())));
        }
        assert!(calls.lock().unwrap().is_empty(), "the request holds the callbacks");
        drop(req);
        assert_eq!(*calls.lock().unwrap(), [(0, false), (1, false)]);
    }

    #[test]
    fn callbacks_run_when_the_last_copy_of_the_request_extensions_is_dropped() {
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut req = http::Request::new(());
        req.on_finish({
            let called = called.clone();
            move |_| called.store(true, std::sync::atomic::Ordering::SeqCst)
        });
        let extensions = req.extensions().clone();
        drop(req);
        assert!(
            !called.load(std::sync::atomic::Ordering::SeqCst),
            "a copy still holds the callback"
        );
        drop(extensions);
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn a_panicking_callback_does_not_prevent_the_others() {
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut req = http::Request::new(());
        req.on_finish(|_| panic!("the callback panicked"));
        req.on_finish({
            let called = called.clone();
            move |_| called.store(true, std::sync::atomic::Ordering::SeqCst)
        });
        drop(req);
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }
}
