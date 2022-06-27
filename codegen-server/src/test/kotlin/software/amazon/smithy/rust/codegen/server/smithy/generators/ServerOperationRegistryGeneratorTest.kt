/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.util.lookup

class ServerOperationRegistryGeneratorTest {
    private val model = """
        namespace test
        
        use aws.protocols#restJson1

        @restJson1
        service Service {
            operations: [
                Frobnify,
                SayHello,
            ],
        }

        /// Only the Frobnify operation is documented,
        /// over multiple lines.
        /// And here are #hash #tags!
        @http(method: "GET", uri: "/frobnify")
        operation Frobnify {
            input: FrobnifyInputOutput,
            output: FrobnifyInputOutput,
            errors: [FrobnifyFailure]
        }

        @http(method: "GET", uri: "/hello")
        operation SayHello {
            input: SayHelloInputOutput,
            output: SayHelloInputOutput,
        }

        structure FrobnifyInputOutput {}
        structure SayHelloInputOutput {}

        @error("server")
        structure FrobnifyFailure {}
    """.asSmithyModel()

    @Test
    fun `it generates quickstart example`() {
        val serviceShape = model.lookup<ServiceShape>("test#Service")
        val (protocolShapeId, protocolGeneratorFactory) = ServerProtocolLoader(ServerProtocolLoader.DefaultProtocols).protocolFor(
            model,
            serviceShape
        )
        val serverCodegenContext = serverTestCodegenContext(
            model,
            serviceShape,
            settings = serverTestRustSettings(moduleName = "service"),
            protocolShapeId = protocolShapeId
        )

        val index = TopDownIndex.of(serverCodegenContext.model)
        val operations = index.getContainedOperations(serverCodegenContext.serviceShape).sortedBy { it.id }
        val httpBindingResolver = protocolGeneratorFactory.protocol(serverCodegenContext).httpBindingResolver

        val generator = ServerOperationRegistryGenerator(serverCodegenContext, httpBindingResolver, operations)
        val writer = RustWriter.forModule("operation_registry")
        generator.render(writer)

        writer.toString() shouldContain
                """
                /// ```rust
                /// use std::net::SocketAddr;
                /// use service::{input, output, error};
                /// use service::operation_registry::OperationRegistryBuilder;
                /// use aws_smithy_http_server::routing::Router;
                ///
                /// #[tokio::main]
                /// pub async fn main() {
                ///    let app: Router = OperationRegistryBuilder::default()
                ///        .frobnify(frobnify)
                ///        .say_hello(say_hello)
                ///        .build()
                ///        .expect("unable to build operation registry")
                ///        .into();
                ///
                ///    let bind: SocketAddr = format!("{}:{}", "127.0.0.1", "6969")
                ///        .parse()
                ///        .expect("unable to parse the server bind address and port");
                ///
                ///    let server = hyper::Server::bind(&bind).serve(app.into_make_service());
                ///
                ///    // Run your service!
                ///    // if let Err(err) = server.await {
                ///    //   eprintln!("server error: {}", err);
                ///    // }
                /// }
                ///
                /// /// Only the Frobnify operation is documented,
                /// /// over multiple lines.
                /// /// And here are #hash #tags!
                /// async fn frobnify(input: input::FrobnifyInputOutput) -> Result<output::FrobnifyInputOutput, error::FrobnifyError> {
                ///     todo!()
                /// }
                ///
                /// async fn say_hello(input: input::SayHelloInputOutput) -> output::SayHelloInputOutput {
                ///     todo!()
                /// }
                /// ```
                ///""".trimIndent()
    }
}
