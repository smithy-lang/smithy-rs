/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::event_stream::Receiver;
use aws_smithy_runtime_api::box_error::BoxError;

#[derive(Debug)]
/// Receives unmarshalled events at a time out of an Event Stream.
pub struct EventReceiver<T, E> {
    inner: Receiver<T, E>,
}

impl<T, E> EventReceiver<T, E> {
    pub(crate) fn new(inner: Receiver<T, E>) -> Self {
        Self { inner }
    }

    /// Asynchronously tries to receive an event from the stream. If the stream has ended, it
    /// returns an `Ok(None)`. If there is an error, such as failing to unmarshall a message in
    /// the stream, it returns an [`BoxError`].
    pub async fn recv(&mut self) -> Result<Option<T>, BoxError>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        self.inner.recv().await.map_err(Into::into)
    }
}
