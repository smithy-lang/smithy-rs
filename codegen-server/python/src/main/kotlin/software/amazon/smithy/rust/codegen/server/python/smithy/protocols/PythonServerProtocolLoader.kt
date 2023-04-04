
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserSection
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerAwsJsonFactory
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolCustomization
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolSection
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerRestJsonFactory

/**
 * Customization class used to force casting a non primitive type into one overriden by a new symbol provider,
 * by explicitly calling `from()` on it.
 *
 * For example we use this in the server Python implementation, where we override types like [Blob], [DateTime] and [Document]
 * with wrappers compatible with Python, without touching the original implementation coming from `aws-smithy-types`.
 */
class PythonServerAfterDeserializedMemberJsonParserCustomization(private val runtimeConfig: RuntimeConfig) :
    JsonParserCustomization() {
    override fun section(section: JsonParserSection): Writable = when (section) {
        is JsonParserSection.AfterTimestampDeserializedMember -> writable {
            rust(".map(#T::from)", PythonServerRuntimeType.dateTime(runtimeConfig).toSymbol())
        }
        is JsonParserSection.AfterBlobDeserializedMember -> writable {
            rust(".map(#T::from)", PythonServerRuntimeType.blob(runtimeConfig).toSymbol())
        }
        is JsonParserSection.AfterDocumentDeserializedMember -> writable {
            rust(".map(#T::from)", PythonServerRuntimeType.document(runtimeConfig).toSymbol())
        }
        else -> writable {}
    }
}

/**
 * Customization class used to force casting a non primitive type into one overriden by a new symbol provider,
 * by explicitly calling `into()` on it.
 */
class PythonServerAfterDeserializedMemberHttpBoundCustomization() :
    ServerHttpBoundProtocolCustomization() {
    override fun section(section: ServerHttpBoundProtocolSection): Writable = when (section) {
        is ServerHttpBoundProtocolSection.AfterTimestampDeserializedMember -> writable {
            rust(".into()")
        }
    }
}

class PythonServerProtocolLoader(
    private val supportedProtocols: ProtocolMap<ServerProtocolGenerator, ServerCodegenContext>,
) : ProtocolLoader<ServerProtocolGenerator, ServerCodegenContext>(supportedProtocols) {

    companion object {
        fun defaultProtocols(runtimeConfig: RuntimeConfig) =
            mapOf(
                RestJson1Trait.ID to ServerRestJsonFactory(
                    additionalParserCustomizations = listOf(
                        PythonServerAfterDeserializedMemberJsonParserCustomization(runtimeConfig),
                    ),
                    additionalHttpBoundCustomizations = listOf(
                        PythonServerAfterDeserializedMemberHttpBoundCustomization(),
                    ),
                ),
                AwsJson1_0Trait.ID to ServerAwsJsonFactory(
                    AwsJsonVersion.Json10,
                    additionalParserCustomizations = listOf(
                        PythonServerAfterDeserializedMemberJsonParserCustomization(runtimeConfig),
                    ),
                    additionalHttpBoundCustomizations = listOf(
                        PythonServerAfterDeserializedMemberHttpBoundCustomization(),
                    ),
                ),
                AwsJson1_1Trait.ID to ServerAwsJsonFactory(
                    AwsJsonVersion.Json11,
                    additionalParserCustomizations = listOf(
                        PythonServerAfterDeserializedMemberJsonParserCustomization(runtimeConfig),
                    ),
                    additionalHttpBoundCustomizations = listOf(
                        PythonServerAfterDeserializedMemberHttpBoundCustomization(),
                    ),
                ),
            )
    }
}
