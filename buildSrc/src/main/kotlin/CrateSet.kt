/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

object CrateSet {
    val AWS_SDK_RUNTIME = listOf(
        "aws-config",
        "aws-credential-types",
        "aws-endpoint",
        "aws-http",
        "aws-hyper",
        "aws-runtime",
        "aws-runtime-api",
        "aws-sig-auth",
        "aws-sigv4",
        "aws-types",
    )

    private val SMITHY_RUNTIME_COMMON = listOf(
        "aws-smithy-async",
        "aws-smithy-checksums",
        "aws-smithy-client",
        "aws-smithy-eventstream",
        "aws-smithy-http",
        "aws-smithy-http-auth",
        "aws-smithy-http-tower",
        "aws-smithy-json",
        "aws-smithy-protocol-test",
        "aws-smithy-query",
        "aws-smithy-runtime",
        "aws-smithy-runtime-api",
        "aws-smithy-types",
        "aws-smithy-types-convert",
        "aws-smithy-xml",
    )

    val AWS_SDK_SMITHY_RUNTIME = SMITHY_RUNTIME_COMMON

    val SERVER_SMITHY_RUNTIME = SMITHY_RUNTIME_COMMON + listOf(
        "aws-smithy-http-server",
        "aws-smithy-http-server-python",
        "aws-smithy-http-server-typescript",
    )

    val ENTIRE_SMITHY_RUNTIME = (AWS_SDK_SMITHY_RUNTIME + SERVER_SMITHY_RUNTIME).toSortedSet()
}
