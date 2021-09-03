/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeCrateLocation
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.crateLocation
import java.io.File
import java.nio.file.Path

fun RuntimeConfig.awsRoot(): RuntimeCrateLocation = when (runtimeCrateLocation) {
    is RuntimeCrateLocation.Path -> {
        val cratePath = (runtimeCrateLocation as RuntimeCrateLocation.Path).path
        val asPath = Path.of(cratePath)
        val path = if (asPath.isAbsolute) {
            asPath.parent.resolve("aws/rust-runtime").toAbsolutePath().toString()
        } else {
            cratePath
        }
        check(File(path).exists()) { "$path must exist to generate a working SDK" }
        RuntimeCrateLocation.Path(path)
    }
    is RuntimeCrateLocation.Versioned -> runtimeCrateLocation
}

object AwsRuntimeType {
    val S3Errors by lazy { RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("s3_errors")) }
    val Presigning by lazy {
        RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("presigning", public = true))
    }
}

fun RuntimeConfig.awsRuntimeDependency(name: String, features: Set<String> = setOf()): CargoDependency =
    CargoDependency(name, awsRoot().crateLocation(), features = features)
