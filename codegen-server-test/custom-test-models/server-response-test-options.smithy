$version: "2"

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
namespace smithy.rust.test

/// Additional assertions for server response protocol tests.
structure ResponseTestOptions {
    /// Reject response headers absent from headers and requireHeaders.
    forbidAdditionalHeaders: Boolean
}
