/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node

// generated as part of the build, see codegen-core/build.gradle.kts
private const val VERSION_FILENAME = "runtime-crate-version.txt"

data class Version(
    val fullVersion: String,
    val stableCrateVersion: String,
    val unstableCrateVersion: String,
    val crates: Map<String, String>,
) {
    companion object {
        // Version must be in the "{smithy_rs_version}\n{git_commit_hash}" format
        fun parse(content: String): Version {
            val node = Node.parse(content).expectObjectNode()
            val githash = node.expectStringMember("githash").value
            val stableVersion = node.expectStringMember("stableVersion").value
            val unstableVersion = node.expectStringMember("unstableVersion").value
            return Version(
                "$stableVersion-$githash",
                stableCrateVersion = stableVersion,
                unstableCrateVersion = unstableVersion,
                node.expectObjectMember("runtimeCrates").members.map {
                    it.key.value to it.value.expectStringNode().value
                }.toMap(),
            )
        }

        // Returns full version in the "{smithy_rs_version}-{git_commit_hash}" format
        fun fullVersion(): String =
            fromDefaultResource().fullVersion

        fun stableCrateVersion(): String =
            fromDefaultResource().stableCrateVersion

        fun unstableCrateVersion(): String =
            fromDefaultResource().unstableCrateVersion

        fun crateVersion(crate: String): String {
            val version = fromDefaultResource()
            return version.crates[crate] ?: version.unstableCrateVersion
        }
        fun fromDefaultResource(): Version = parse(
            Version::class.java.getResource(VERSION_FILENAME)?.readText()
                ?: throw CodegenException("$VERSION_FILENAME does not exist"),
        )
    }
}
