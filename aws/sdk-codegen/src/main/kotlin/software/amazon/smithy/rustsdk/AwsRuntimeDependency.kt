/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.crateLocation
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
        path = updatedPath, version = runtimeCrateLocation.version?.let { defaultSdkVersion() }
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
            CargoDependency.SmithyHttp(this),
            CargoDependency.SmithyHttpTower(this),
            CargoDependency.SmithyClient(this),
            CargoDependency.Tower,
            awsHttp(),
            awsEndpoint(),
        )
    ).member("DefaultMiddleware")
}

fun RuntimeConfig.awsRuntimeDependency(name: String, features: Set<String> = setOf()): CargoDependency =
    CargoDependency(name, awsRoot().crateLocation(), features = features)

fun RuntimeConfig.awsHttp(): CargoDependency = awsRuntimeDependency("aws-http")
fun RuntimeConfig.awsTypes(): CargoDependency = awsRuntimeDependency("aws-types")
fun RuntimeConfig.awsConfig(): CargoDependency = awsRuntimeDependency("aws-config")
fun RuntimeConfig.awsEndpoint() = awsRuntimeDependency("aws-endpoint")
