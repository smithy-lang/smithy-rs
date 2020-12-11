package software.amazon.smithy.rust.codegen.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.testutil.testSymbolProvider

class HttpProtocolTestGeneratorTest {
    private val baseModel = """
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
    private val model = OperationNormalizer(baseModel).transformModel(
        inputBodyFactory = OperationNormalizer.NoBody,
        outputBodyFactory = OperationNormalizer.NoBody
    )
    private val symbolProvider = testSymbolProvider(model)
    private val runtimeConfig = TestRuntimeConfig
    private val correctBody = """{"name": "Teddy"}"""

    /**
     * Creates an fake HTTP implementation for SayHello & generates the protocol test
     */
    private fun writeHttpImpl(
        writer: RustWriter,
        httpRequestBuilder: String,
        body: String = "${correctBody.dq()}.to_string()",
        correctResponse: String = """Ok(SayHelloOutput::builder().value("hey there!").build())"""
    ) {
        val shape: StructureShape = model.lookup("com.example#SayHelloInput")
        val inputSymbol = symbolProvider.toSymbol(shape)
        val operationShape: OperationShape = model.lookup("com.example#SayHello")
        val builderGenerator = OperationInputBuilderGenerator(model, symbolProvider, model.lookup("com.example#SayHello"))
        writer.withModule("error") {
            StructureGenerator(model, symbolProvider, this, model.lookup("com.example#BadRequest")).render()
            CombinedErrorGenerator(model, symbolProvider, operationShape).render(this)
        }
        writer.withModule("operation") {
            rustBlock("pub struct SayHello") {
                write("input: #T", inputSymbol)
            }
            implBlock(operationShape, symbolProvider) {
                builderGenerator.renderConvenienceMethod(this)

                rustBlock(
                    "pub fn build_http_request(&self) -> #T<Vec<u8>>", RuntimeType.Http("request::Request")
                ) {
                    write("#T::assemble(self.input.request_builder_base(), self.input.build_body())", inputSymbol)
                }

                rustBlock("pub fn new(input: #T) -> Self", inputSymbol) {
                    write("Self { input }")
                }

                rustBlock(
                    "pub fn from_response(_response: #T<impl AsRef<[u8]>>) -> Result<#T, #T>",

                    RuntimeType.Http("response::Response"),
                    symbolProvider.toSymbol(operationShape.outputShape(model)),
                    operationShape.errorSymbol(symbolProvider)
                ) {
                    writeWithNoFormatting(correctResponse)
                }
            }
        }
        writer.withModule("output") {
            val outputShape = operationShape.outputShape(model)
            outputShape.renderWithModelBuilder(model, symbolProvider, this)
        }
        writer.withModule("input") {
            StructureGenerator(model, symbolProvider, this, shape).render()
            builderGenerator.render(this)
            rustBlock("impl SayHelloInput") {
                builderGenerator.renderConvenienceMethod(this)
                rustBlock("pub fn request_builder_base(&self) -> #T", RuntimeType.HttpRequestBuilder) {
                    write("#T::new()", RuntimeType.HttpRequestBuilder)
                    write(httpRequestBuilder)
                }
                rustBlock("pub fn build_body(&self) -> String") {
                    write(body)
                }
                rustBlock(
                    "pub fn assemble<T: Into<#3T>>(builder: #1T, body: T) -> #2T<#3T>",
                    RuntimeType.HttpRequestBuilder,
                    RuntimeType.Http("request::Request"),
                    RuntimeType.ByteSlab
                ) {
                    write("let body = body.into();")
                    write("builder.header(#T, body.len()).body(body)", RuntimeType.Http("header::CONTENT_LENGTH"))
                    write(""".expect("http request should be valid")""")
                }
            }
            val protocolConfig = ProtocolConfig(
                model,
                symbolProvider,
                runtimeConfig,
                model.lookup("com.example#HelloService"),
                RestJson1Trait.ID
            )
            HttpProtocolTestGenerator(
                protocolConfig,
                ProtocolSupport(
                    requestBodySerialization = true,
                    responseDeserialization = true,
                    errorDeserialization = false
                ),
                model.lookup("com.example#SayHello"),
                this
            ).render()
        }
    }

    @Test
    fun `passing e2e protocol request test`() {
        val writer = RustWriter.root()
        writeHttpImpl(
            writer,
            """
                    .uri("/?Hi=Hello%20there&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                """
        )

        val testOutput = writer.compileAndTest()
        // Verify the test actually ran
        testOutput shouldContain "say_hello_request ... ok"
    }

    @Test
    fun `test incorrect response parsing`() {
        val writer = RustWriter.root()
        writeHttpImpl(
            writer,
            """
                    .uri("/?Hi=Hello%20there&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                """,
            correctResponse = "Ok(SayHelloOutput::builder().build())"
        )
        val err = assertThrows<CommandFailed> {
            writer.compileAndTest(expectFailure = true)
        }

        err.message shouldContain "basic_response_test_response ... FAILED"
    }

    @Test
    fun `test invalid body`() {
        val writer = RustWriter.root()
        writeHttpImpl(
            writer,
            """
                    .uri("/?Hi=Hello%20there&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                """,
            """
                    "{}".to_string()
"""
        )

        val err = assertThrows<CommandFailed> {
            writer.compileAndTest(expectFailure = true)
        }

        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "body did not match"
    }

    @Test
    fun `test invalid url parameter`() {
        val writer = RustWriter.root()

        // Hard coded implementation for this 1 test
        writeHttpImpl(
            writer,
            """
                    .uri("/?Hi=INCORRECT&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                """
        )

        val err = assertThrows<CommandFailed> {
            writer.compileAndTest(expectFailure = true)
        }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "missing query param"
    }

    @Test
    fun `test forbidden url parameter`() {
        val writer = RustWriter.root()

        // Hard coded implementation for this 1 test
        writeHttpImpl(
            writer,
            """
                    .uri("/?goodbye&Hi=Hello%20there&required")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                """
        )

        val err = assertThrows<CommandFailed> {
            writer.compileAndTest(expectFailure = true)
        }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "forbidden query param"
    }

    @Test
    fun `test required url parameter`() {
        val writer = RustWriter.root()

        // Hard coded implementation for this 1 test
        writeHttpImpl(
            writer,
            """
                    .uri("/?Hi=Hello%20there")
                    .header("X-Greeting", "Hi")
                    .method("POST")
                """
        )

        val err = assertThrows<CommandFailed> {
            writer.compileAndTest(expectFailure = true)
        }
        // Verify the test actually ran
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "required query param missing"
    }

    @Test
    fun `invalid header`() {
        val writer = RustWriter.root()
        writeHttpImpl(
            writer,
            """
                    .uri("/?Hi=Hello%20there&required")
                    // should be "Hi"
                    .header("X-Greeting", "Hey")
                    .method("POST")
                """
        )

        val err = assertThrows<CommandFailed> {
            writer.compileAndTest(expectFailure = true)
        }
        err.message shouldContain "say_hello_request ... FAILED"
        err.message shouldContain "invalid header value"
    }
}
