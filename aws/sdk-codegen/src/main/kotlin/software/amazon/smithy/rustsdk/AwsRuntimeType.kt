/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import java.io.File
import java.nio.file.Path

fun RuntimeConfig.awsRoot(): RuntimeCrateLocation {
    val updatedPath =
        runtimeCrateLocation.path?.let { cratePath ->
            val asPath = Path.of(cratePath)
            val path =
                if (asPath.isAbsolute) {
                    asPath.parent.resolve("aws/rust-runtime").toAbsolutePath().toString()
                } else {
                    cratePath
                }
            check(File(path).exists()) { "$path must exist to generate a working SDK" }
            path
        }
    return runtimeCrateLocation.copy(
        path = updatedPath, versions = runtimeCrateLocation.versions,
    )
}

object AwsRuntimeType {
    fun presigning(): RuntimeType =
        RuntimeType.forInlineDependency(
            InlineAwsDependency.forRustFile(
                "presigning", visibility = Visibility.PUBLIC,
                CargoDependency.Http1x,
                CargoDependency.HttpBody1x,
            ),
        )

    fun presigningInterceptor(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineDependency(
            InlineAwsDependency.forRustFile(
                "presigning_interceptors",
                visibility = Visibility.PUBCRATE,
                AwsCargoDependency.awsSigv4(runtimeConfig),
                CargoDependency.smithyRuntimeApiClient(runtimeConfig),
            ),
        )

    fun awsCredentialTypes(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsCredentialTypes(runtimeConfig).toType()

    fun awsCredentialTypesTestUtil(runtimeConfig: RuntimeConfig) =
        AwsCargoDependency.awsCredentialTypes(runtimeConfig).toDevDependency().withFeature("test-util").toType()

    fun awsSigv4(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsSigv4(runtimeConfig).toType()

    fun awsTypes(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsTypes(runtimeConfig).toType()

    fun awsRuntime(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsRuntime(runtimeConfig).toType()

    fun awsRuntimeTestUtil(runtimeConfig: RuntimeConfig) =
        AwsCargoDependency.awsRuntime(runtimeConfig).toDevDependency().withFeature("test-util").toType()

    fun awsRuntimeApi(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsRuntimeApi(runtimeConfig).toType()
}
