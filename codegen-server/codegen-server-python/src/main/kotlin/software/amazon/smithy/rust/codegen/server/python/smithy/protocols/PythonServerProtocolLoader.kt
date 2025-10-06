
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
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingSection
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
    override fun section(section: JsonParserSection): Writable =
        when (section) {
            is JsonParserSection.AfterTimestampDeserializedMember ->
                writable {
                    rust(".map(#T::from)", PythonServerRuntimeType.dateTime(runtimeConfig).toSymbol())
                }
            is JsonParserSection.AfterBlobDeserializedMember ->
                writable {
                    rust(".map(#T::from)", PythonServerRuntimeType.blob(runtimeConfig).toSymbol())
                }
            is JsonParserSection.AfterDocumentDeserializedMember ->
                writable {
                    rust(".map(#T::from)", PythonServerRuntimeType.document(runtimeConfig).toSymbol())
                }
            else -> emptySection
        }
}

/**
 * Customization class used to force casting a non-primitive type into one overridden by a new symbol provider,
 * by explicitly calling `into()` on it.
 */
class PythonServerAfterDeserializedMemberServerHttpBoundCustomization :
    ServerHttpBoundProtocolCustomization() {
    override fun section(section: ServerHttpBoundProtocolSection): Writable =
        when (section) {
            is ServerHttpBoundProtocolSection.AfterTimestampDeserializedMember ->
                writable {
                    rust(".into()")
                }

            else -> emptySection
        }
}

/**
 * Customization class used to force casting a `Vec<DateTime>` into one a Python `Vec<DateTime>`
 */
class PythonServerAfterDeserializedMemberHttpBindingCustomization(private val runtimeConfig: RuntimeConfig) :
    HttpBindingCustomization() {
    override fun section(section: HttpBindingSection): Writable =
        when (section) {
            is HttpBindingSection.AfterDeserializingIntoADateTimeOfHttpHeaders ->
                writable {
                    rust(".into_iter().map(#T::from).collect()", PythonServerRuntimeType.dateTime(runtimeConfig).toSymbol())
                }
            else -> emptySection
        }
}

/**
 * Customization class used to determine how serialized stream payload should be rendered for the Python server.
 *
 * In this customization, we do not need to wrap the payload in a new-type wrapper to enable the
 * `futures_core::stream::Stream` trait since the payload in question has a type
 * `aws_smithy_http_server_python::types::ByteStream` which already implements the `Stream` trait.
 */
class PythonServerStreamPayloadSerializerCustomization() : ServerHttpBoundProtocolCustomization() {
    override fun section(section: ServerHttpBoundProtocolSection): Writable =
        when (section) {
            is ServerHttpBoundProtocolSection.WrapStreamPayload ->
                writable {
                    section.params.payloadGenerator.generatePayload(this, section.params.shapeName, section.params.shape)
                }

            else -> emptySection
        }
}

class PythonServerProtocolLoader(
    private val supportedProtocols: ProtocolMap<ServerProtocolGenerator, ServerCodegenContext>,
) : ProtocolLoader<ServerProtocolGenerator, ServerCodegenContext>(supportedProtocols) {
    companion object {
        fun defaultProtocols(runtimeConfig: RuntimeConfig) =
            mapOf(
                RestJson1Trait.ID to
                    ServerRestJsonFactory(
                        additionalParserCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberJsonParserCustomization(runtimeConfig),
                            ),
                        additionalServerHttpBoundProtocolCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberServerHttpBoundCustomization(),
                                PythonServerStreamPayloadSerializerCustomization(),
                            ),
                        additionalHttpBindingCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberHttpBindingCustomization(runtimeConfig),
                            ),
                    ),
                AwsJson1_0Trait.ID to
                    ServerAwsJsonFactory(
                        AwsJsonVersion.Json10,
                        additionalParserCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberJsonParserCustomization(runtimeConfig),
                            ),
                        additionalServerHttpBoundProtocolCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberServerHttpBoundCustomization(),
                                PythonServerStreamPayloadSerializerCustomization(),
                            ),
                        additionalHttpBindingCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberHttpBindingCustomization(runtimeConfig),
                            ),
                    ),
                AwsJson1_1Trait.ID to
                    ServerAwsJsonFactory(
                        AwsJsonVersion.Json11,
                        additionalParserCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberJsonParserCustomization(runtimeConfig),
                            ),
                        additionalServerHttpBoundProtocolCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberServerHttpBoundCustomization(),
                                PythonServerStreamPayloadSerializerCustomization(),
                            ),
                        additionalHttpBindingCustomizations =
                            listOf(
                                PythonServerAfterDeserializedMemberHttpBindingCustomization(runtimeConfig),
                            ),
                    ),
            )
    }
}
