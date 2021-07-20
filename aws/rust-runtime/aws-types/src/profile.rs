/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

mod source;

// exposed only to remove unused code warnings until the parser side is added
#[doc(hidden)]
pub use source::load;
