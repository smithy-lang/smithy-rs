/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

data class Crate(val name: String, val versionPropertyName: String)

object CrateSet {
    const val STABLE_VERSION_PROP_NAME = "smithy.rs.runtime.crate.stable.version"
    private const val UNSTABLE_VERSION_PROP_NAME = "smithy.rs.runtime.crate.unstable.version"

    /*
     * Crates marked as `STABLE_VERSION_PROP_NAME` should have the following package metadata in their `Cargo.toml`
     *
     * [package.metadata.release-tooling]
     * stable = true
     */

    val AWS_SDK_RUNTIME = listOf(
        Crate("aws-config", STABLE_VERSION_PROP_NAME),
        Crate("aws-credential-types", STABLE_VERSION_PROP_NAME),
        Crate("aws-endpoint", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-http", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-hyper", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-runtime", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-runtime-api", STABLE_VERSION_PROP_NAME),
        Crate("aws-sig-auth", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-sigv4", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-types", STABLE_VERSION_PROP_NAME),
    )

    private val SMITHY_RUNTIME_COMMON = listOf(
        Crate("aws-smithy-async", STABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-checksums", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-client", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-eventstream", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-http", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-http-auth", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-http-tower", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-json", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-protocol-test", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-query", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-runtime", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-runtime-api", STABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-types", STABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-types-convert", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-xml", UNSTABLE_VERSION_PROP_NAME),
    )

    val AWS_SDK_SMITHY_RUNTIME = SMITHY_RUNTIME_COMMON

    private val SERVER_SMITHY_RUNTIME = SMITHY_RUNTIME_COMMON + listOf(
        Crate("aws-smithy-http-server", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-http-server-python", UNSTABLE_VERSION_PROP_NAME),
        Crate("aws-smithy-http-server-typescript", UNSTABLE_VERSION_PROP_NAME),
    )

    val ENTIRE_SMITHY_RUNTIME = (AWS_SDK_SMITHY_RUNTIME + SERVER_SMITHY_RUNTIME).toSortedSet(compareBy { it.name })
}
