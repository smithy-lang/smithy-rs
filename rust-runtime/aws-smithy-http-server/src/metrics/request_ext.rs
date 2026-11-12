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
    /// standard metrics are disabled. Callbacks are invoked in the order they were registered.
    ///
    /// This method should be called from a [Layer] or [Plugin] configured via the service's server
    /// SDK. If this method is called from elsewhere, such as from a [Layer] applied around the
    /// service rather than through its configuration, the result is a no-op, and the callback will
    /// never be invoked.
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
        // `&mut self` not really necessary, but keeping it to avoid tying our hands later.
        match self.extensions().get::<MetricsState>() {
            Some(state) => state.push_callback(Box::new(callback)),
            None => tracing::debug!("request has no metrics state; `on_finish` callback will not be invoked"),
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

    use super::*;

    #[test]
    fn registers_callbacks_in_order() {
        let state = MetricsState::new(true, &Default::default());
        let mut req = http::Request::new(());
        req.extensions_mut().insert(state.clone());
        req.on_finish(|_| {});
        req.on_finish(|_| {});
        assert_eq!(state.callback_count(), 2);
    }

    #[test]
    fn registers_callbacks_when_disabled() {
        let state = MetricsState::new(false, &Default::default());
        let mut req = http::Request::new(());
        req.extensions_mut().insert(state.clone());
        req.on_finish(|_| {});
        assert_eq!(state.callback_count(), 1);
    }

    #[test]
    fn drops_callbacks_without_state() {
        let captured = Arc::new(());
        let mut req = http::Request::new(());
        req.on_finish({
            let captured = captured.clone();
            move |_| drop(captured)
        });
        // The callback, and the clone it captured, were dropped rather than retained.
        assert_eq!(Arc::strong_count(&captured), 1);
    }
}
