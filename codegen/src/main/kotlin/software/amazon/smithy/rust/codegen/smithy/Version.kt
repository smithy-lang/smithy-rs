/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.CodegenException

// generated as part of the build, see codegen/build.gradle.kts
private const val VERSION_FILENAME = "runtime-crate-version.txt"

data class Version(val fullVersion: String, val crateVersion: String) {
    companion object {
        // Version must be in the "{smithy_rs_version}\n{git_commit_hash}" format
        fun parse(content: String): Version {
            val lines = content.lines()
            if (lines.size != 2) {
                throw IllegalArgumentException("Invalid version format, it should contain `2` lines but contains `${lines.size}` line(s)")
            }
            return Version(lines.joinToString("-"), lines.first())
        }

        // Returns full version in the "{smithy_rs_version}-{git_commit_hash}" format
        fun fullVersion(): String =
            fromDefaultResource().fullVersion

        fun crateVersion(): String =
            fromDefaultResource().crateVersion

        private fun fromDefaultResource(): Version =
            parse(
                object {}.javaClass.getResource(VERSION_FILENAME)?.readText()
                    ?: throw CodegenException("$VERSION_FILENAME does not exist"),
            )
    }
}
