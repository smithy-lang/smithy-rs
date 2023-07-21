/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.generators.http.HttpBindingCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.smithy.protocols.awsJsonFieldName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.JsonParserCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSerializerSection
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customizations.BeforeIteratingOverMapOrCollectionJsonCustomization
import software.amazon.smithy.rust.codegen.server.smithy.customizations.BeforeSerializingMemberJsonCustomization
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerAwsJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol

/**
 * AwsJson 1.0 and 1.1 server-side protocol factory. This factory creates the [ServerHttpBoundProtocolGenerator]
 * with AwsJson specific configurations.
 */
class ServerAwsJsonFactory(
    private val version: AwsJsonVersion,
    private val additionalParserCustomizations: List<JsonParserCustomization> = listOf(),
    private val additionalServerHttpBoundProtocolCustomizations: List<ServerHttpBoundProtocolCustomization> = listOf(),
    private val additionalHttpBindingCustomizations: List<HttpBindingCustomization> = listOf(),
) : ProtocolGeneratorFactory<ServerHttpBoundProtocolGenerator, ServerCodegenContext> {
    override fun protocol(codegenContext: ServerCodegenContext): ServerProtocol =
        ServerAwsJsonProtocol(codegenContext, version, additionalParserCustomizations)

    override fun buildProtocolGenerator(codegenContext: ServerCodegenContext): ServerHttpBoundProtocolGenerator =
        ServerHttpBoundProtocolGenerator(
            codegenContext,
            protocol(codegenContext),
            additionalServerHttpBoundProtocolCustomizations,
            additionalHttpBindingCustomizations,
        )

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            /* Client support */
            requestSerialization = false,
            requestBodySerialization = false,
            responseDeserialization = false,
            errorDeserialization = false,
            /* Server support */
            requestDeserialization = true,
            requestBodyDeserialization = true,
            responseSerialization = true,
            errorSerialization = true,
        )
    }
}

/**
 * AwsJson requires errors to be serialized in server responses with an additional `__type` field. This
 * customization writes the right field depending on the version of the AwsJson protocol.
 *
 * From the specs:
 * - https://smithy.io/2.0/aws/protocols/aws-json-1_0-protocol.html#operation-error-serialization
 * - https://smithy.io/2.0/aws/protocols/aws-json-1_1-protocol.html#operation-error-serialization
 *
 * > Error responses in the <awsJson1_x> protocol are serialized identically to standard responses with one additional
 * > component to distinguish which error is contained. New server-side protocol implementations SHOULD use a body
 * > field named __type
 */
class ServerAwsJsonError(private val awsJsonVersion: AwsJsonVersion) : JsonSerializerCustomization() {
    override fun section(section: JsonSerializerSection): Writable = when (section) {
        is JsonSerializerSection.ServerError -> writable {
            if (section.structureShape.hasTrait<ErrorTrait>()) {
                val typeId = when (awsJsonVersion) {
                    // AwsJson 1.0 wants the whole shape ID (namespace#Shape).
                    // https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html#operation-error-serialization
                    AwsJsonVersion.Json10 -> section.structureShape.id.toString()
                    // AwsJson 1.1 wants only the shape name (Shape).
                    // https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_1-protocol.html#operation-error-serialization
                    AwsJsonVersion.Json11 -> section.structureShape.id.name.toString()
                }
                rust("""${section.jsonObject}.key("__type").string("${escape(typeId)}");""")
            }
        }

        else -> emptySection
    }
}

/**
 * AwsJson requires operation errors to be serialized in server response with an additional `__type` field. This class
 * customizes [JsonSerializerGenerator] to add this functionality.
 *
 * https://awslabs.github.io/smithy/1.0/spec/aws/aws-json-1_0-protocol.html#operation-error-serialization
 */
class ServerAwsJsonSerializerGenerator(
    private val codegenContext: ServerCodegenContext,
    private val httpBindingResolver: HttpBindingResolver,
    private val awsJsonVersion: AwsJsonVersion,
    private val jsonSerializerGenerator: JsonSerializerGenerator =
        JsonSerializerGenerator(
            codegenContext,
            httpBindingResolver,
            ::awsJsonFieldName,
            customizations = listOf(
                ServerAwsJsonError(awsJsonVersion),
                BeforeIteratingOverMapOrCollectionJsonCustomization(codegenContext),
                BeforeSerializingMemberJsonCustomization(codegenContext),
            ),
        ),
) : StructuredDataSerializerGenerator by jsonSerializerGenerator
