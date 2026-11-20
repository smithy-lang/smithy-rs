/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{
    state::{MetricsState, PendingCallbacks},
    RequestMetrics,
};

/// Extension methods on [`http::Request`] for accessing standard server metrics.
///
/// This trait is sealed and cannot be implemented outside this crate.
pub trait RequestExt: sealed::Sealed {
    /// Registers a callback to be invoked once the request is finished, meaning the response has
    /// been fully sent or sending it has failed.
    ///
    /// The callback receives the standard server metrics collected for the request, or `None` if
    /// standard metrics are disabled. Callbacks are invoked in the order they were registered.
    ///
    /// This method may be called from a [Layer] or [Plugin] configured via the service's server
    /// SDK, or from a [Layer] applied around the service. A callback registered before the request
    /// reaches the service is invoked once the request finishes, like any other. If the request
    /// never reaches the Smithy service, the callback is never invoked.
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
            // The service takes over pending callbacks when the request reaches it.
            None => self
                .extensions_mut()
                .get_or_insert_default::<PendingCallbacks>()
                .push(Box::new(callback)),
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
        let state = MetricsState::new(
            true,
            &Default::default(),
            &ServiceIdentity::of::<TestService>(),
            0,
            Vec::new(),
        );
        let mut req = http::Request::new(());
        req.extensions_mut().insert(state.clone());
        req.on_finish(|_| {});
        req.on_finish(|_| {});
        assert_eq!(state.callback_count(), 2);
    }

    #[test]
    fn registers_callbacks_when_disabled() {
        let state = MetricsState::new(
            false,
            &Default::default(),
            &ServiceIdentity::of::<TestService>(),
            0,
            Vec::new(),
        );
        let mut req = http::Request::new(());
        req.extensions_mut().insert(state.clone());
        req.on_finish(|_| {});
        assert_eq!(state.callback_count(), 1);
    }

    #[test]
    fn callbacks_without_state_are_held_until_the_request_is_dropped() {
        let captured = Arc::new(());
        let mut req = http::Request::new(());
        req.on_finish({
            let captured = captured.clone();
            move |_| drop(captured)
        });
        assert_eq!(Arc::strong_count(&captured), 2, "the request holds the callback");
        drop(req);
        assert_eq!(
            Arc::strong_count(&captured),
            1,
            "a request that never reaches a service drops its callbacks"
        );
    }
}
