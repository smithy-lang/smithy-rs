/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Conversions from Stream-like structs to implementors of `futures::Stream`

use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_stream::Stream;

use aws_smithy_async::future::pagination_stream::PaginationStream;

/// Stream implementor wrapping `PaginationStream`
pub struct PaginationStreamImplStream<Item> {
    pagination_stream: PaginationStream<Item>,
}

impl<Item> PaginationStreamImplStream<Item>
where
    Item: Send + 'static,
{
    /// Create a new Stream object wrapping a `PaginationStream`
    pub fn new(pagination_stream: PaginationStream<Item>) -> Self {
        PaginationStreamImplStream { pagination_stream }
    }
}

impl<Item> Stream for PaginationStreamImplStream<Item>
where
    Item: Send + 'static,
{
    type Item = Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.pagination_stream.poll_next(cx)
    }
}
