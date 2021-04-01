/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Local
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import java.nio.file.Path

fun RuntimeConfig.awsRoot(): String {
    val asPath = Path.of(relativePath)
    return if (asPath.isAbsolute) {
        asPath.parent.resolve("aws/rust-runtime").toAbsolutePath().toString()
    } else {
        relativePath
    }
}

fun RuntimeConfig.awsRuntimeDependency(name: String, features: List<String> = listOf()): CargoDependency =
    CargoDependency(name, Local(awsRoot()), features = features)
