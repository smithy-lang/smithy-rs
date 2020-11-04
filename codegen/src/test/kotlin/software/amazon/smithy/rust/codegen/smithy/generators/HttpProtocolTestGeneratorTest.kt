package software.amazon.smithy.rust.codegen.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.testutil.asSmithy
import software.amazon.smithy.rust.testutil.shouldCompile
import software.amazon.smithy.rust.testutil.testSymbolProvider

class HttpProtocolTestGeneratorTest {
    val baseModel = """
        namespace com.example

        use aws.protocols#restJson1
        use smithy.test#httpRequestTests

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
                headers: {
                    "X-Greeting": "Hi",
                },
                body: "{\"name\": \"Teddy\"}",
                bodyMediaType: "application/json"
            }
        ])
        operation SayHello {
            input: SayHelloInput
        }

        structure SayHelloInput {
            @httpHeader("X-Greeting")
            greeting: String,

            @httpQuery("Hi")
            query: String,

            name: String
        }
    """.asSmithy()
    private val model = OperationNormalizer(testSymbolProvider(baseModel)).addOperationInputs(baseModel)
    private val symbolProvider = testSymbolProvider(model)
    private val runtimeConfig = TestRuntimeConfig

    private fun fakeInput(writer: RustWriter, body: String) {
        StructureGenerator(model, symbolProvider, writer, model.lookup("com.example#SayHelloInput")).render()
        writer.rustBlock("impl SayHelloInput") {
            rustBlock("pub fn build_http_request(&self) -> \$T", RuntimeType.HttpRequestBuilder) {
                write("\$T::new()", RuntimeType.HttpRequestBuilder)
                write(body)
            }
        }
    }

    @Test
    fun `passing e2e protocol request test`() {
        val writer = RustWriter.forModule("lib")

        // Hard coded implementation for this 1 test
        writer.withModule("operation") {
            fakeInput(
                this,
                """
                        .uri("/?Hi=Hello%20there")
                        .header("X-Greeting", "Hi")
                        .method("POST")
                    """
            )
            val protocolConfig = ProtocolConfig(
                model,
                symbolProvider,
                runtimeConfig,
                this,
                model.lookup("com.example#HelloService"),
                model.lookup("com.example#SayHello"),
                model.lookup("com.example#SayHelloInput"),
                RestJson1Trait.ID
            )
            HttpProtocolTestGenerator(
                protocolConfig
            ).render()
        }

        val testOutput = writer.shouldCompile()
        // Verify the test actually ran
        testOutput shouldContain "test_say_hello ... ok"
    }

    @Test
    fun `failing e2e protocol test`() {
        val writer = RustWriter.forModule("lib")

        // Hard coded implementation for this 1 test
        writer.withModule("operation") {
            fakeInput(
                this,
                """
                        .uri("/?Hi=INCORRECT")
                        .header("X-Greeting", "Hi")
                        .method("POST")
                    """
            )
            val protocolConfig = ProtocolConfig(
                model,
                symbolProvider,
                runtimeConfig,
                this,
                model.lookup("com.example#HelloService"),
                model.lookup("com.example#SayHello"),
                model.lookup("com.example#SayHelloInput"),
                RestJson1Trait.ID
            )
            HttpProtocolTestGenerator(
                protocolConfig
            ).render()
        }

        val err = assertThrows<CommandFailed> {
            writer.shouldCompile(expectFailure = true)
        }
        // Verify the test actually ran
        err.message shouldContain "test_say_hello ... FAILED"
        err.message shouldContain "MissingQueryParam"
    }
}
