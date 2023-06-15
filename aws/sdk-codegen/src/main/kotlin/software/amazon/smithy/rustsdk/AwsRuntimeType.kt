/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import java.io.File
import java.nio.file.Path

fun defaultSdkVersion(): String {
    // generated as part of the build, see codegen/build.gradle.kts
    try {
        return object {}.javaClass.getResource("sdk-crate-version.txt")?.readText()
            ?: throw CodegenException("sdk-crate-version.txt does not exist")
    } catch (ex: Exception) {
        throw CodegenException("failed to load sdk-crate-version.txt which sets the default client-runtime version", ex)
    }
}

fun RuntimeConfig.awsRoot(): RuntimeCrateLocation {
    val updatedPath = runtimeCrateLocation.path?.let { cratePath ->
        val asPath = Path.of(cratePath)
        val path = if (asPath.isAbsolute) {
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
        RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("presigning", visibility = Visibility.PUBLIC))
    fun presigningInterceptor(runtimeConfig: RuntimeConfig): RuntimeType =
        RuntimeType.forInlineDependency(
            InlineAwsDependency.forRustFile(
                "presigning_interceptors",
                visibility = Visibility.PUBCRATE,
                AwsCargoDependency.awsSigv4(runtimeConfig),
                CargoDependency.smithyRuntimeApi(runtimeConfig),
            ),
        )

    // TODO(enableNewSmithyRuntimeCleanup): Delete the `presigning_service.rs` inlineable when cleaning up middleware
    fun presigningService(): RuntimeType =
        RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("presigning_service", visibility = Visibility.PUBCRATE))

    // TODO(enableNewSmithyRuntimeCleanup): Delete defaultMiddleware and middleware.rs, and remove tower dependency from inlinables, when cleaning up middleware
    fun RuntimeConfig.defaultMiddleware() = RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "middleware", visibility = Visibility.PUBLIC,
            CargoDependency.smithyHttp(this),
            CargoDependency.smithyHttpTower(this),
            CargoDependency.smithyClient(this),
            CargoDependency.Tower,
            AwsCargoDependency.awsSigAuth(this),
            AwsCargoDependency.awsHttp(this),
            AwsCargoDependency.awsEndpoint(this),
        ),
    ).resolve("DefaultMiddleware")

    fun awsCredentialTypes(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsCredentialTypes(runtimeConfig).toType()

    fun awsCredentialTypesTestUtil(runtimeConfig: RuntimeConfig) =
        AwsCargoDependency.awsCredentialTypes(runtimeConfig).toDevDependency().withFeature("test-util").toType()

    fun awsEndpoint(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsEndpoint(runtimeConfig).toType()
    fun awsHttp(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsHttp(runtimeConfig).toType()
    fun awsSigAuth(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsSigAuth(runtimeConfig).toType()
    fun awsSigAuthEventStream(runtimeConfig: RuntimeConfig) =
        AwsCargoDependency.awsSigAuthEventStream(runtimeConfig).toType()

    fun awsSigv4(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsSigv4(runtimeConfig).toType()
    fun awsTypes(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsTypes(runtimeConfig).toType()

    fun awsRuntime(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsRuntime(runtimeConfig).toType()
    fun awsRuntimeApi(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsRuntimeApi(runtimeConfig).toType()
}
