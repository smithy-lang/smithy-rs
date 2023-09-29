/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Useful runtime-agnostic future implementations.

pub mod never;
pub mod now_or_later;
pub mod pagination_stream;
pub mod rendezvous;
pub mod timeout;
