/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

/**
 * HTTP version identifier for dual-version testing.
 *
 * This enum distinguishes between HTTP 0.x (hyper 0.x) and HTTP 1.x (hyper 1.x)
 * when running integration tests that need to verify behavior across both versions.
 */
enum class HttpTestVersion(val displayName: String) {
    /** HTTP 0.x using hyper 0.x */
    HTTP_0_X("HTTP 0.x"),

    /** HTTP 1.x using hyper 1.x */
    HTTP_1_X("HTTP 1.x"),
}
