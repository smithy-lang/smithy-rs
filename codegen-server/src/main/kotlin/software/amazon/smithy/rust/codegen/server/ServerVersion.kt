/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node

private const val VERSION_FILENAME = "server-codegen-version.json"

internal data class ServerVersion(val codegenVersion: String, val gitHash: String) {
    companion object {
        fun parse(content: String): ServerVersion {
            val node = Node.parse(content).expectObjectNode()
            return ServerVersion(node.expectStringMember("codegenVersion").value, node.expectStringMember("gitHash").value)
        }

        fun fromDefaultResource(): ServerVersion =
            parse(
                ServerVersion::class.java.getResource(VERSION_FILENAME)?.readText()
                    ?: throw CodegenException("$VERSION_FILENAME does not exist"),
            )
    }
}
