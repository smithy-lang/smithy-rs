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
import software.amazon.smithy.rust.codegen.core.smithy.crateLocation
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
    val S3Errors by lazy { RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("s3_errors")) }
    val Presigning by lazy {
        RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("presigning", visibility = Visibility.PUBLIC))
    }

    fun RuntimeConfig.defaultMiddleware() = RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "middleware", visibility = Visibility.PUBLIC,
            CargoDependency.smithyHttp(this),
            CargoDependency.smithyHttpTower(this),
            CargoDependency.smithyClient(this),
            CargoDependency.Tower,
        ),
    ).resolve("DefaultMiddleware")

    fun awsEndpoint(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsEndpoint(runtimeConfig).toType()
    fun awsHttp(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsHttp(runtimeConfig).toType()
    fun awsSigAuth(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsSigAuth(runtimeConfig).toType()
    fun awsSigAuthEventStream(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsSigAuthEventStream(runtimeConfig).toType()
    fun awsSigv4(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsSigv4(runtimeConfig).toType()
    fun awsTypes(runtimeConfig: RuntimeConfig) = AwsCargoDependency.awsTypes(runtimeConfig).toType()
}

fun RuntimeConfig.awsRuntimeCrate(name: String, features: Set<String> = setOf()): CargoDependency =
    CargoDependency(name, awsRoot().crateLocation(null), features = features)

object AwsCargoDependency {
    fun awsConfig(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-config")
    fun awsEndpoint(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-endpoint")
    fun awsHttp(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-http")
    fun awsSigAuth(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-sig-auth")
    fun awsSigAuthEventStream(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-sig-auth", setOf("sign-eventstream"))
    fun awsSigv4(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-sigv4")
    fun awsTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-types")
}
