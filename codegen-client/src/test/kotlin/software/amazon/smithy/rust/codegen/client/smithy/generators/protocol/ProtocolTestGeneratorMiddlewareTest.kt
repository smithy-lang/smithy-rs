/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.HttpBoundProtocolTraitImplGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.AdditionalPayloadContext
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.util.outputShape

private class TestProtocolPayloadGenerator(private val body: String) : ProtocolPayloadGenerator {
    override fun payloadMetadata(operationShape: OperationShape, additionalPayloadContext: AdditionalPayloadContext) =
        ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = false)

    override fun generatePayload(
        writer: RustWriter,
        shapeName: String,
        operationShape: OperationShape,
        additionalPayloadContext: AdditionalPayloadContext,
    ) {
        writer.writeWithNoFormatting(body)
    }
}

private class TestProtocolTraitImplGenerator(
    private val codegenContext: ClientCodegenContext,
    private val correctResponse: String,
) : HttpBoundProtocolTraitImplGenerator(codegenContext, RestJson(codegenContext)) {
    private val symbolProvider = codegenContext.symbolProvider

    override fun generateTraitImpls(
        operationWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        operationWriter.rustTemplate(
            """
            impl #{parse_strict} for ${operationShape.id.name}{
                type Output = Result<#{Output}, #{Error}>;
                fn parse(&self, _response: &#{Response}<#{Bytes}>) -> Self::Output {
                    ${operationWriter.escape(correctResponse)}
                }
            }
            """,
            "parse_strict" to RuntimeType.parseStrictResponse(codegenContext.runtimeConfig),
            "Output" to symbolProvider.toSymbol(operationShape.outputShape(codegenContext.model)),
            "Error" to symbolProvider.symbolForOperationError(operationShape),
            "Response" to RuntimeType.HttpResponse,
            "Bytes" to RuntimeType.Bytes,
        )
    }
}

private class TestProtocolMakeOperationGenerator(
    codegenContext: CodegenContext,
    protocol: Protocol,
    body: String,
    private val httpRequestBuilder: String,
) : MakeOperationGenerator(
    codegenContext,
    protocol,
    TestProtocolPayloadGenerator(body),
    public = true,
    includeDefaultPayloadHeaders = true,
) {
    override fun createHttpRequest(writer: RustWriter, operationShape: OperationShape) {
        writer.rust("#T::new()", RuntimeType.HttpRequestBuilder)
        writer.writeWithNoFormatting(httpRequestBuilder)
    }
}

// A stubbed test protocol to do enable testing intentionally broken protocols
private class TestProtocolGenerator(
    codegenContext: ClientCodegenContext,
    protocol: Protocol,
    httpRequestBuilder: String,
    body: String,
    correctResponse: String,
) : OperationGenerator(
    codegenContext,
    protocol,
    TestProtocolMakeOperationGenerator(codegenContext, protocol, body, httpRequestBuilder),
    TestProtocolPayloadGenerator(body),
    TestProtocolTraitImplGenerator(codegenContext, correctResponse),
)

private class TestProtocolFactory(
    private val httpRequestBuilder: String,
    private val body: String,
    private val correctResponse: String,
) : ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol = RestJson(codegenContext)

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): OperationGenerator {
        return TestProtocolGenerator(
            codegenContext,
            protocol(codegenContext),
            httpRequestBuilder,
            body,
            correctResponse,
        )
    }

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            requestSerialization = true,
            requestBodySerialization = true,
            responseDeserialization = true,
            errorDeserialization = true,
            requestDeserialization = false,
            requestBodyDeserialization = false,
            responseSerialization = false,
            errorSerialization = false,
        )
    }
}
