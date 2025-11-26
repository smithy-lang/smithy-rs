/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

data class Crate(val name: String, val versionPropertyName: String)

object CrateSet {
    const val STABLE_VERSION_PROP_NAME = "smithy.rs.runtime.crate.stable.version"
    const val UNSTABLE_VERSION_PROP_NAME = "smithy.rs.runtime.crate.unstable.version"

    /*
     * Crates marked as `STABLE_VERSION_PROP_NAME` should have the following package metadata in their `Cargo.toml`
     *
     * [package.metadata.smithy-rs-release-tooling]
     * stable = true
     */

    val StableCrates =
        setOf(
            // AWS crates
            "aws-config",
            "aws-credential-types",
            "aws-runtime",
            "aws-runtime-api",
            "aws-sigv4",
            "aws-types",
            // smithy crates
            "aws-smithy-async",
            "aws-smithy-runtime-api",
            "aws-smithy-runtime",
            "aws-smithy-types",
            "aws-smithy-http-client",
        )

    val version = { name: String ->
        when {
            StableCrates.contains(name) -> STABLE_VERSION_PROP_NAME
            else -> UNSTABLE_VERSION_PROP_NAME
        }
    }

    // If we make changes to `AWS_SDK_RUNTIME`, also update the list in
    // https://github.com/smithy-lang/smithy-rs/blob/main/tools/ci-build/sdk-lockfiles/src/audit.rs#L22
    val AWS_SDK_RUNTIME =
        listOf(
            "aws-config",
            "aws-credential-types",
            "aws-runtime",
            "aws-runtime-api",
            "aws-sigv4",
            "aws-types",
            "aws-sdk-cloudfront-url-signer",
        ).map { Crate(it, version(it)) }

    val SMITHY_RUNTIME_COMMON =
        listOf(
            "aws-smithy-async",
            "aws-smithy-cbor",
            "aws-smithy-checksums",
            "aws-smithy-compression",
            "aws-smithy-dns",
            "aws-smithy-eventstream",
            "aws-smithy-experimental",
            "aws-smithy-http",
            "aws-smithy-http-client",
            "aws-smithy-json",
            "aws-smithy-legacy-http",
            "aws-smithy-mocks",
            "aws-smithy-observability",
            "aws-smithy-observability-otel",
            "aws-smithy-protocol-test",
            "aws-smithy-query",
            "aws-smithy-runtime",
            "aws-smithy-runtime-api",
            "aws-smithy-types",
            "aws-smithy-types-convert",
            "aws-smithy-wasm",
            "aws-smithy-xml",
        ).map { Crate(it, version(it)) }

    val AWS_SDK_SMITHY_RUNTIME = SMITHY_RUNTIME_COMMON

    // If we make changes to `SERVER_SPECIFIC_SMITHY_RUNTIME`, also update the list in
    // https://github.com/smithy-lang/smithy-rs/blob/main/tools/ci-build/sdk-lockfiles/src/audit.rs#L38
    private val SERVER_SPECIFIC_SMITHY_RUNTIME =
        listOf(
            Crate("aws-smithy-http-server", UNSTABLE_VERSION_PROP_NAME),
            Crate("aws-smithy-http-server-python", UNSTABLE_VERSION_PROP_NAME),
            Crate("aws-smithy-http-server-typescript", UNSTABLE_VERSION_PROP_NAME),
            Crate("aws-smithy-legacy-http-server", UNSTABLE_VERSION_PROP_NAME),
        )

    val SERVER_SMITHY_RUNTIME = SMITHY_RUNTIME_COMMON + SERVER_SPECIFIC_SMITHY_RUNTIME

    val ENTIRE_SMITHY_RUNTIME = (AWS_SDK_SMITHY_RUNTIME + SERVER_SMITHY_RUNTIME).toSortedSet(compareBy { it.name })

    val ALL_CRATES = AWS_SDK_RUNTIME + ENTIRE_SMITHY_RUNTIME
}
