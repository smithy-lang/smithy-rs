//! Conversions from Stream-like structs to implementors of `futures::Stream`

use aws_smithy_async::future::pagination_stream::PaginationStream;
use futures::lock::Mutex;
use futures::*;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Stream implementor wrapping `PaginationStream`
pub struct FuturesPaginationStream<Item> {
    pagination_stream: Arc<Mutex<PaginationStream<Item>>>,
    next_future: Option<Pin<Box<dyn Future<Output = Option<Item>> + Send>>>,
}

impl<Item> FuturesPaginationStream<Item>
where
    Item: Send + 'static,
{
    /// Create a new Stream object wrapping a `PaginationStream`
    pub fn new(pagination_stream: PaginationStream<Item>) -> Self {
        FuturesPaginationStream {
            pagination_stream: Arc::new(Mutex::new(pagination_stream)),
            next_future: None,
        }
    }

    fn set_next_future(mut self: Pin<&mut Self>) {
        let pagination_stream = self.pagination_stream.clone();
        let future = Box::pin(async move {
            let mut stream = pagination_stream.lock().await;
            stream.next().await
        });
        self.next_future = Some(future);
    }
}

impl<Item> Stream for FuturesPaginationStream<Item>
where
    Item: Send + 'static,
{
    type Item = Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.next_future.is_none() {
            self.as_mut().set_next_future();
        }

        let next_future = self.next_future.as_mut().unwrap();
        match next_future.as_mut().poll(cx) {
            Poll::Ready(next_item) => {
                self.next_future = None;
                Poll::Ready(next_item)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
