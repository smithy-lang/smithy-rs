/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

// TODO(enableNewSmithyRuntimeCleanup): Delete this file

// Map an ALPN protocol ID to a version from the `http` Rust crate
private fun mapHttpVersion(httpVersion: String): String {
    return when (httpVersion) {
        "http/1.1" -> "http::Version::HTTP_11"
        "h2" -> "http::Version::HTTP_2"
        else -> TODO("Unsupported HTTP version '$httpVersion', please check your model")
    }
}
