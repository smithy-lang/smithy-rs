/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc::error::SendError;
use tokio::sync::Semaphore;

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let semaphore = Arc::new(Semaphore::new(0));
    (
        Sender {
            semaphore: semaphore.clone(),
            chan: tx,
        },
        Receiver {
            semaphore: semaphore.clone(),
            chan: rx,
            needs_permit: false,
        },
    )
}

pub struct Sender<T> {
    semaphore: Arc<Semaphore>,
    chan: tokio::sync::mpsc::Sender<T>,
}

impl<T> Sender<T> {
    pub async fn send(&self, item: T) -> Result<(), SendError<T>> {
        let result = self.chan.send(item).await;
        // The key here is that we block _after_ the send until more demand exists
        self.semaphore
            .acquire()
            .await
            .expect("semaphore is never closed")
            .forget();
        result
    }
}

pub struct Receiver<T> {
    semaphore: Arc<Semaphore>,
    chan: tokio::sync::mpsc::Receiver<T>,
    needs_permit: bool,
}

impl<T> Receiver<T> {
    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let resp = self.chan.poll_recv(cx);
        // If there is no data on the channel, but we are reading, then give a permit so we can load data
        if self.needs_permit && matches!(resp, Poll::Pending) {
            self.needs_permit = false;
            self.semaphore.add_permits(1);
        }

        if matches!(resp, Poll::Ready(_)) {
            // we returned an item, no need to provide another permit until we fail to read from the channel again
            self.needs_permit = true;
        }
        resp
    }
}
