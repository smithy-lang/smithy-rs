/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! CBOR abstractions for Smithy.

pub mod data;
pub mod decode;
pub mod encode;

pub use decode::Decoder;
pub use encode::Encoder;
