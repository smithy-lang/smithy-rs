/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Provides Sender/Receiver implementations for Event Stream codegen.

mod receiver;
mod sender;

pub use crate::BoxError;

#[doc(inline)]
pub use sender::{EventStreamSender, MessageStreamAdapter, MessageStreamError};

#[doc(inline)]
pub use receiver::{Error, RawMessage, Receiver};
