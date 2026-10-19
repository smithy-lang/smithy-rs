/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node

// generated as part of the build, see codegen-core/build.gradle.kts
private const val VERSION_FILENAME = "runtime-crate-versions.json"

data class Version(
    val gitHash: String,
    val crates: Map<String, String>,
) {
    companion object {
        // Version must be in the "{smithy_rs_version}\n{git_commit_hash}" format
        fun parse(content: String): Version {
            val node = Node.parse(content).expectObjectNode()
            return Version(
                node.expectStringMember("gitHash").value,
                node.expectObjectMember("runtimeCrates").members.map {
                    it.key.value to it.value.expectStringNode().value
                }.toMap(),
            )
        }

        fun crateVersion(crate: String): String {
            val version = fromDefaultResource()
            return version.crates[crate] ?: throw CodegenException("unknown version number for runtime crate $crate")
        }

        fun fromDefaultResource(): Version =
            parse(
                Version::class.java.getResource(VERSION_FILENAME)?.readText()
                    ?: throw CodegenException("$VERSION_FILENAME does not exist"),
            )
    }
}
