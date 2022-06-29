/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.protocol

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.escape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CodegenVisitor
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.runCommand
import java.nio.file.Path

private class TestProtocolPayloadGenerator(private val body: String) : ProtocolPayloadGenerator {
    override fun payloadMetadata(operationShape: OperationShape) =
        ProtocolPayloadGenerator.PayloadMetadata(takesOwnership = false)

    override fun generatePayload(writer: RustWriter, self: String, operationShape: OperationShape) {
        writer.writeWithNoFormatting(body)
    }
}

private class TestProtocolTraitImplGenerator(
    private val coreCodegenContext: CoreCodegenContext,
    private val correctResponse: String
) : ProtocolTraitImplGenerator {
    private val symbolProvider = coreCodegenContext.symbolProvider

    override fun generateTraitImpls(operationWriter: RustWriter, operationShape: OperationShape) {
        operationWriter.rustTemplate(
            """
            impl #{parse_strict} for ${operationShape.id.name}{
                type Output = Result<#{output}, #{error}>;
                fn parse(&self, _response: &#{response}<#{bytes}>) -> Self::Output {
                    ${operationWriter.escape(correctResponse)}
                }
                    }""",
            "parse_strict" to RuntimeType.parseStrictResponse(coreCodegenContext.runtimeConfig),
            "output" to symbolProvider.toSymbol(operationShape.outputShape(coreCodegenContext.model)),
            "error" to operationShape.errorSymbol(symbolProvider),
            "response" to RuntimeType.Http("Response"),
            "bytes" to RuntimeType.Bytes
        )
    }
}

private class TestProtocolMakeOperationGenerator(
    coreCodegenContext: CoreCodegenContext,
    protocol: Protocol,
    body: String,
    private val httpRequestBuilder: String
) : MakeOperationGenerator(
    coreCodegenContext,
    protocol,
    TestProtocolPayloadGenerator(body),
    public = true,
    includeDefaultPayloadHeaders = true
) {
    override fun createHttpRequest(writer: RustWriter, operationShape: OperationShape) {
        writer.rust("#T::new()", RuntimeType.HttpRequestBuilder)
        writer.writeWithNoFormatting(httpRequestBuilder)
    }
}

// A stubbed test protocol to do enable testing intentionally broken protocols
private class TestProtocolGenerator(
    coreCodegenContext: CoreCodegenContext,
    protocol: Protocol,
    httpRequestBuilder: String,
    body: String,
    correctResponse: String
) : ProtocolGenerator(
    coreCodegenContext,
    protocol,
    TestProtocolMakeOperationGenerator(coreCodegenContext, protocol, body, httpRequestBuilder),
    TestProtocolTraitImplGenerator(coreCodegenContext, correctResponse)
)

private class TestProtocolFactory(
    private val httpRequestBuilder: String,
    private val body: String,
    private val correctResponse: String
) : ProtocolGeneratorFactory<ProtocolGenerator, ClientCodegenContext> {
    override fun protocol(codegenContext: ClientCodegenContext): Protocol {
        return RestJson(codegenContext)
    }

    override fun buildProtocolGenerator(codegenContext: ClientCodegenContext): ProtocolGenerator {
        return TestProtocolGenerator(
            codegenContext,
            protocol(codegenContext),
            httpRequestBuilder,
            body,
            correctResponse
        )
    }

    override fun transformModel(model: Model): Model = model

    override fun support(): ProtocolSupport {
        return ProtocolSupport(
            requestSerialization = true,
            requestBodySerialization = true,
            responseDeserialization = true,
            errorDeserialization = true,
            requestDeserialization = false,
            requestBodyDeserialization = false,
            responseSerialization = false,
            errorSerialization = false
        )
    }
}

class ProtocolTestGeneratorTest {
    private val model = """
        namespace com.example

        use aws.protocols#restJson1
        use smithy.test#httpRequestTests
        use smithy.test#httpResponseTests

        @restJson1
        service HelloService {
            operations: [SayHello],
            version: "1"
        }

        @http(method: "POST", uri: "/")
        @httpRequestTests([
            {
                id: "say_hello",
                protocol: restJson1,
                params: {
                    "greeting": "Hi",
                    "name": "Teddy",
                    "query": "Hello there"
                },
                method: "POST",
                uri: "/",
                queryParams: [
                    "Hi=Hello%20there"
                ],
                forbidQueryParams: [
                    "goodbye"
                ],
                requireQueryParams: ["required"],
                headers: {
                    "X-Greeting": "Hi",
                },
                body: "{\"name\": \"Teddy\"}",
                bodyMediaType: "application/json"
            }
        ])
        @httpResponseTests([{
            id: "basic_response_test",
            protocol: restJson1,
            documentation: "Parses operations with empty JSON bodies",
            body: "{\"value\": \"hey there!\"}",
            params: {"value": "hey there!"},
            bodyMediaType: "application/json",
            headers: {"Content-Type": "application/x-amz-json-1.1"},
            code: 200,
        }])
        operation SayHello {
            input: SayHelloInput,
            output: SayHelloOutput,
            errors: [BadRequest]
        }

        structure SayHelloOutput {
            value: String
        }

        @error("client")
        structure BadRequest {
            message: String
        }

        structure SayHelloInput {
            @httpHeader("X-Greeting")
            greeting: String,

            @httpQuery("Hi")
            query: String,

            name: String
        }
    """.asSmithyModel()
    private val correctBody = """{"name": "Teddy"}"""

    /**
     * Creates a fake HTTP implementation for SayHello & generates the protocol test
     *
     * Returns the [Path] the service was generated at, suitable for running `cargo test`
     */
    private fun generateService(
        httpRequestBuilder: String,
        body: String = "${correctBody.dq()}.to_string()",
        correctResponse: String = """Ok(crate::output::SayHelloOutput::builder().value("hey there!").build())"""
    ): Path {
        val (pluginContext, testDir) = generatePluginContext(model)
        val visitor = CodegenVisitor(
            pluginContext,
            object : RustCodegenDecorator<ClientCodegenContext> {
                override val name: String = "mock"
                override val order: Byte = 0
                override fun protocols(
                    serviceId: ShapeId,
                    currentProtocols: ProtocolMap<ClientCodegenContext>
                ): ProtocolMap<ClientCodegenContext> =
                    // Intentionally replace the builtin implementation of RestJson1 with our fake protocol
                    mapOf(RestJson1Trait.ID to TestProtocolFactory(httpRequestBuilder, body, correctResponse))
            }
        )
        visitor.execute()
        println("file:///$testDir/src/operation.rs")
        return testDir
    }

    @Test
    fun `passing e2e protocol request test`() {
        val path = generateService(
            """
            .uri("/?Hi=Hello%20there&required")
            .header("X-Greeting", "Hi")
            .method("POST")
            """
        )

        val testOutput = "cargo test".runCommand(path)
        // Verify the test actually ran
        testOutput shouldContain "say_hello_request ... ok"
    }

    @Test
    fun `test incorrect response parsing`() {
        val path = generateService(
            """
            .uri("/?Hi=Hello%20there&required")
            .header("X-Greeting", "Hi")
            .method("POST")
            """,
            correctResponse = "Ok(crate::output::SayHelloOutput::builder().build())"
        )
        val err = assertThrows<CommandFailed> {
            "cargo test".runCommand(path)
        }

        err.message shouldContain "basic_response_test_response ... FAILED"
    }

    @Test
    fun `test invalid body`() {
        val path = generateService(
            """
            .uri("/?Hi=Hello%20there&required")
            .header("X-Greeting", "Hi")
            .method("POST")
            """,
            """"{}".to_string()"""
        )

        val err = assertThrows<CommandFailed> {
            "cargo test".runCommand(path)
        }

        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "body did not match"
    }

    @Test
    fun `test invalid url parameter`() {
        // Hard coded implementation for this 1 test
        val path = generateService(
            """
            .uri("/?Hi=INCORRECT&required")
            .header("X-Greeting", "Hi")
            .method("POST")
            """
        )

        val err = assertThrows<CommandFailed> {
            "cargo test".runCommand(path)
        }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "missing query param"
    }

    @Test
    fun `test forbidden url parameter`() {
        val path = generateService(
            """
            .uri("/?goodbye&Hi=Hello%20there&required")
            .header("X-Greeting", "Hi")
            .method("POST")
            """
        )

        val err = assertThrows<CommandFailed> {
            "cargo test".runCommand(path)
        }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "forbidden query param"
    }

    @Test
    fun `test required url parameter`() {
        // Hard coded implementation for this 1 test
        val path = generateService(
            """
            .uri("/?Hi=Hello%20there")
            .header("X-Greeting", "Hi")
            .method("POST")
            """
        )

        val err = assertThrows<CommandFailed> {
            "cargo test".runCommand(path)
        }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "required query param missing"
    }

    @Test
    fun `invalid header`() {
        val path = generateService(
            """
            .uri("/?Hi=Hello%20there&required")
            // should be "Hi"
            .header("X-Greeting", "Hey")
            .method("POST")
            """
        )

        val err = assertThrows<CommandFailed> {
            "cargo test".runCommand(path)
        }
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "invalid header value"
    }
}
