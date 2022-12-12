/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Abstractions for Smithy
//! [XML Binding Traits](https://smithy.io/2.0/spec/protocol-traits.html#xml-bindings)
pub mod decode;
pub mod encode;
mod escape;
mod unescape;
