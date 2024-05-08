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
        )

    val version = { name: String ->
        when {
            StableCrates.contains(name) -> STABLE_VERSION_PROP_NAME
            else -> UNSTABLE_VERSION_PROP_NAME
        }
    }

    val AWS_SDK_RUNTIME =
        listOf(
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
        ).map { Crate(it, version(it)) }

    val SMITHY_RUNTIME_COMMON =
        listOf(
            "aws-smithy-async",
            "aws-smithy-checksums",
            "aws-smithy-compression",
            "aws-smithy-client",
            "aws-smithy-eventstream",
            "aws-smithy-http",
            "aws-smithy-http-auth",
            "aws-smithy-http-tower",
            "aws-smithy-json",
            "aws-smithy-mocks-experimental",
            "aws-smithy-experimental",
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

    val SERVER_SMITHY_RUNTIME =
        SMITHY_RUNTIME_COMMON +
            listOf(
                Crate("aws-smithy-http-server", UNSTABLE_VERSION_PROP_NAME),
                Crate("aws-smithy-http-server-python", UNSTABLE_VERSION_PROP_NAME),
                Crate("aws-smithy-http-server-typescript", UNSTABLE_VERSION_PROP_NAME),
            )

    val ENTIRE_SMITHY_RUNTIME = (AWS_SDK_SMITHY_RUNTIME + SERVER_SMITHY_RUNTIME).toSortedSet(compareBy { it.name })

    val ALL_CRATES = AWS_SDK_RUNTIME + ENTIRE_SMITHY_RUNTIME
}
