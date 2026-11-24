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

    fun awsRuntime(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-runtime")

    fun awsRuntimeApi(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-runtime-api")

    fun awsSigv4(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-sigv4")

    fun awsTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-types")
}
