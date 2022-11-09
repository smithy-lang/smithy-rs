/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.Tower
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.smithyClient
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.smithyHttp
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency.Companion.smithyHttpTower
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
            smithyHttp(this),
            smithyHttpTower(this),
            smithyClient(this),
            Tower,
            awsRuntimeCrate("aws-http"),
            awsRuntimeCrate("aws-endpoint"),
        ),
    ).resolve("DefaultMiddleware")
}

fun RuntimeConfig.awsRuntimeCrate(name: String, features: Set<String> = setOf()): CargoDependency =
    CargoDependency(name, awsRoot().crateLocation(null), features = features)

fun awsEndpoint(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-endpoint").asType()
fun awsHttp(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-http").asType()
fun awsSigAuth(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-sig-auth").asType()
fun awsSigAuthEventStream(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-sig-auth", setOf("sign-eventstream")).asType()
fun awsSigv4(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-sigv4").asType()
fun awsTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeCrate("aws-types").asType()
