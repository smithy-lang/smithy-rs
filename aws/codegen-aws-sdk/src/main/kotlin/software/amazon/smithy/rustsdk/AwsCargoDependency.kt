/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.crateLocation

fun RuntimeConfig.awsRuntimeCrate(
    name: String,
    features: Set<String> = setOf(),
): CargoDependency = CargoDependency(name, awsRoot().crateLocation(name), features = features)

object AwsCargoDependency {
    fun awsConfig(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-config")

    fun awsCredentialTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-credential-types")

    /**
     * `aws-runtime`, declared with `default-features = false`.
     *
     * Its only default is `rustcrypto`, which it forwards to `aws-sigv4`. Leaving the defaults on
     * would add RustCrypto to every generated crate regardless of what the consumer asks for, so
     * `aws-lc-fips` would supplement it rather than substitute for it. The backend arrives instead
     * via the generated crate's default-on `rustcrypto` feature; see `AwsFluentClientDecorator`.
     */
    fun awsRuntime(runtimeConfig: RuntimeConfig) =
        runtimeConfig.awsRuntimeCrate("aws-runtime").copy(defaultFeatures = false)

    fun awsRuntimeApi(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-runtime-api")

    /**
     * `aws-sigv4`, declared with `default-features = false`.
     *
     * Generated service crates declare this crate directly, not only through `aws-runtime`, so the
     * crypto backend has to be selectable from here too. Cargo gives a consumer no way to switch off
     * a transitive crate's default features, so leaving the defaults on would pin every SDK build to
     * the RustCrypto backend regardless of what the consumer asks for -- which is the whole thing
     * the `aws-lc-fips` feature is trying to avoid.
     *
     * The two non-backend defaults are named back explicitly. The backend itself arrives via the
     * generated crate's `rustcrypto` feature, which is on by default -- see `AwsFluentClientDecorator`.
     */
    fun awsSigv4(runtimeConfig: RuntimeConfig) =
        runtimeConfig.awsRuntimeCrate("aws-sigv4")
            .copy(defaultFeatures = false, features = setOf("sign-http", "http1"))

    fun awsTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-types")
}
