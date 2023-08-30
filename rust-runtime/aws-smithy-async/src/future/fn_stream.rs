/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Utility to drive a stream with an async function and a channel.

use crate::future::rendezvous;
use pin_project_lite::pin_project;
use std::fmt;
use std::future::poll_fn;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub mod collect;

pin_project! {
    /// Utility to drive a stream with an async function and a channel.
    ///
    /// The closure is passed a reference to a `Sender` which acts as a rendezvous channel. Messages
    /// sent to the sender will be emitted to the stream. Because the stream is 1-bounded, the function
    /// will not proceed until the stream is read.
    ///
    /// This utility is used by generated paginators to generate a stream of paginated results.
    ///
    /// If `tx.send` returns an error, the function MUST return immediately.
    ///
    /// Note `FnStream` is only `Send` but not `Sync` because `generator` is a boxed future that
    /// is `Send` and returns `()` as output when it is done.
    ///
    /// # Examples
    /// ```no_run
    /// # async fn docs() {
    /// use aws_smithy_async::future::fn_stream::FnStream;
    /// let mut stream = FnStream::new(|tx| Box::pin(async move {
    ///     if let Err(_) = tx.send("Hello!").await {
    ///         return;
    ///     }
    ///     if let Err(_) = tx.send("Goodbye!").await {
    ///         return;
    ///     }
    /// }));
    /// assert_eq!(stream.collect::<Vec<_>>().await, vec!["Hello!", "Goodbye!"]);
    /// # }
    pub struct FnStream<Item> {
        #[pin]
        rx: rendezvous::Receiver<Item>,
        generator: Option<Pin<Box<dyn Future<Output = ()> + Send + 'static>>>,
    }
}

impl<Item> fmt::Debug for FnStream<Item> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let item_typename = std::any::type_name::<Item>();
        write!(f, "FnStream<{item_typename}>")
    }
}

impl<Item> FnStream<Item> {
    /// Creates a new function based stream driven by `generator`.
    ///
    /// For examples, see the documentation for [`FnStream`]
    pub fn new<T>(generator: T) -> Self
    where
        T: FnOnce(rendezvous::Sender<Item>) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    {
        let (tx, rx) = rendezvous::channel::<Item>();
        Self {
            rx,
            generator: Some(Box::pin(generator(tx))),
        }
    }

    /// Creates unreadable `FnStream` but useful to pass to `std::mem::swap` when extracting an
    /// owned `FnStream` from a mutable reference.
    pub fn taken() -> Self {
        Self::new(|_tx| Box::pin(async move {}))
    }

    /// Consumes and returns the next `Item` from this stream.
    pub async fn next(&mut self) -> Option<Item>
    where
        Self: Unpin,
    {
        let mut me = Pin::new(self);
        poll_fn(|cx| me.as_mut().poll_next(cx)).await
    }

    /// Attempts to pull out the next value of this stream, returning `None` if the stream is
    /// exhausted.
    pub fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Item>> {
        let mut me = self.project();
        match me.rx.poll_recv(cx) {
            Poll::Ready(item) => Poll::Ready(item),
            Poll::Pending => {
                if let Some(generator) = me.generator {
                    if generator.as_mut().poll(cx).is_ready() {
                        // if the generator returned ready we MUST NOT poll it again—doing so
                        // will cause a panic.
                        *me.generator = None;
                    }
                }
                Poll::Pending
            }
        }
    }

    /// Consumes this stream and gathers elements into a collection.
    pub async fn collect<T: collect::sealed::Collectable<Item>>(mut self) -> T {
        let mut collection = T::initialize();
        while let Some(item) = self.next().await {
            if !T::extend(&mut collection, item) {
                break;
            }
        }
        T::finalize(collection)
    }
}

impl<T, E> FnStream<Result<T, E>> {
    /// Yields the next item in the stream or returns an error if an error is encountered.
    pub async fn try_next(&mut self) -> Result<Option<T>, E> {
        self.next().await.transpose()
    }
}

/// Utility wrapper to flatten paginated results
///
/// When flattening paginated results, it's most convenient to produce an iterator where the `Result`
/// is present in each item. This provides `items()` which can wrap an stream of `Result<Page, Err>`
/// and produce a stream of `Result<Item, Err>`.
#[derive(Debug)]
pub struct TryFlatMap<Page, Err>(FnStream<Result<Page, Err>>);

impl<Page, Err> TryFlatMap<Page, Err> {
    /// Creates a `TryFlatMap` that wraps the input.
    pub fn new(stream: FnStream<Result<Page, Err>>) -> Self {
        Self(stream)
    }

    /// Produces a new [`FnStream`] by mapping this stream with `map` then flattening the result.
    pub fn flat_map<M, Item, Iter>(mut self, map: M) -> FnStream<Result<Item, Err>>
    where
        Page: Send + 'static,
        Err: Send + 'static,
        M: Fn(Page) -> Iter + Send + 'static,
        Item: Send + 'static,
        Iter: IntoIterator<Item = Item> + Send,
        <Iter as IntoIterator>::IntoIter: Send,
    {
        FnStream::new(|tx| {
            Box::pin(async move {
                while let Some(page) = self.0.next().await {
                    match page {
                        Ok(page) => {
                            let mapped = map(page);
                            for item in mapped.into_iter() {
                                let _ = tx.send(Ok(item)).await;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            break;
                        }
                    }
                }
            }) as Pin<Box<dyn Future<Output = ()> + Send>>
        })
    }
}

#[cfg(test)]
mod test {
    use crate::future::fn_stream::{FnStream, TryFlatMap};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// basic test of FnStream functionality
    #[tokio::test]
    async fn fn_stream_returns_results() {
        tokio::time::pause();
        let mut stream = FnStream::new(|tx| {
            Box::pin(async move {
                tx.send("1").await.expect("failed to send");
                tokio::time::sleep(Duration::from_secs(1)).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                tx.send("2").await.expect("failed to send");
                tokio::time::sleep(Duration::from_secs(1)).await;
                tx.send("3").await.expect("failed to send");
            })
        });
        let mut out = vec![];
        while let Some(value) = stream.next().await {
            out.push(value);
        }
        assert_eq!(vec!["1", "2", "3"], out);
    }

    #[tokio::test]
    async fn fn_stream_try_next() {
        tokio::time::pause();
        let mut stream = FnStream::new(|tx| {
            Box::pin(async move {
                tx.send(Ok(1)).await.unwrap();
                tx.send(Ok(2)).await.unwrap();
                tx.send(Err("err")).await.unwrap();
            })
        });
        let mut out = vec![];
        while let Ok(value) = stream.try_next().await {
            out.push(value);
        }
        assert_eq!(vec![Some(1), Some(2)], out);
    }

    // smithy-rs#1902: there was a bug where we could continue to poll the generator after it
    // had returned Poll::Ready. This test case leaks the tx half so that the channel stays open
    // but the send side generator completes. By calling `poll` multiple times on the resulting future,
    // we can trigger the bug and validate the fix.
    #[tokio::test]
    async fn fn_stream_doesnt_poll_after_done() {
        let mut stream = FnStream::new(|tx| {
            Box::pin(async move {
                assert!(tx.send("blah").await.is_ok());
                Box::leak(Box::new(tx));
            })
        });
        assert_eq!(Some("blah"), stream.next().await);
        let mut test_stream = tokio_test::task::spawn(stream);
        let _ = test_stream.enter(|ctx, pin| {
            let polled = pin.poll_next(ctx);
            assert!(polled.is_pending());
        });
        let _ = test_stream.enter(|ctx, pin| {
            let polled = pin.poll_next(ctx);
            assert!(polled.is_pending());
        });
    }

    /// Tests that the generator will not advance until demand exists
    #[tokio::test]
    async fn waits_for_reader() {
        let progress = Arc::new(Mutex::new(0));
        let mut stream = FnStream::new(|tx| {
            let progress = progress.clone();
            Box::pin(async move {
                *progress.lock().unwrap() = 1;
                tx.send("1").await.expect("failed to send");
                *progress.lock().unwrap() = 2;
                tx.send("2").await.expect("failed to send");
                *progress.lock().unwrap() = 3;
                tx.send("3").await.expect("failed to send");
                *progress.lock().unwrap() = 4;
            })
        });
        assert_eq!(*progress.lock().unwrap(), 0);
        stream.next().await.expect("ready");
        assert_eq!(*progress.lock().unwrap(), 1);

        assert_eq!("2", stream.next().await.expect("ready"));
        assert_eq!(2, *progress.lock().unwrap());

        let _ = stream.next().await.expect("ready");
        assert_eq!(3, *progress.lock().unwrap());
        assert_eq!(None, stream.next().await);
        assert_eq!(4, *progress.lock().unwrap());
    }

    #[tokio::test]
    async fn generator_with_errors() {
        let mut stream = FnStream::new(|tx| {
            Box::pin(async move {
                for i in 0..5 {
                    if i != 2 {
                        if tx.send(Ok(i)).await.is_err() {
                            return;
                        }
                    } else {
                        tx.send(Err(i)).await.unwrap();
                        return;
                    }
                }
            })
        });
        let mut out = vec![];
        while let Some(Ok(value)) = stream.next().await {
            out.push(value);
        }
        assert_eq!(vec![0, 1], out);
    }

    #[tokio::test]
    async fn flatten_items_ok() {
        #[derive(Debug)]
        struct Output {
            items: Vec<u8>,
        }
        let stream = FnStream::new(|tx| {
            Box::pin(async move {
                tx.send(Ok(Output {
                    items: vec![1, 2, 3],
                }))
                .await
                .unwrap();
                tx.send(Ok(Output {
                    items: vec![4, 5, 6],
                }))
                .await
                .unwrap();
            })
        });
        assert_eq!(
            Ok(vec![1, 2, 3, 4, 5, 6]),
            TryFlatMap::new(stream)
                .flat_map(|output| output.items.into_iter())
                .collect::<Result<Vec<_>, &str>>()
                .await,
        );
    }

    #[tokio::test]
    async fn flatten_items_error() {
        #[derive(Debug)]
        struct Output {
            items: Vec<u8>,
        }
        let stream = FnStream::new(|tx| {
            Box::pin(async move {
                tx.send(Ok(Output {
                    items: vec![1, 2, 3],
                }))
                .await
                .unwrap();
                tx.send(Err("bummer")).await.unwrap();
            })
        });
        assert_eq!(
            Err("bummer"),
            TryFlatMap::new(stream)
                .flat_map(|output| output.items.into_iter())
                .collect::<Result<Vec<_>, &str>>()
                .await
        )
    }
}
