/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.CodegenException

// generated as part of the build in the "{smithy_rs_version}\n{git_commit_hash}" format,
// see codegen/build.gradle.kts
private const val VERSION_FILENAME = "runtime-crate-version.txt"

class Version(private val content: String) {
    // Returns full version in the "{smithy_rs_version}-{git_commit_hash}" format
    fun fullVersion(): String = content.lines().joinToString("-")

    fun crateVersion(): String = content.lines().first()

    companion object {
        fun fullVersion(): String =
            fromDefaultResource().fullVersion()

        fun crateVersion(): String =
            fromDefaultResource().crateVersion()

        private fun fromDefaultResource(): Version =
            Version(
                object {}.javaClass.getResource(VERSION_FILENAME)?.readText()
                    ?: throw CodegenException("$VERSION_FILENAME does not exist"),
            )
    }
}
