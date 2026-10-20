/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import kotlin.io.path.readText

internal class ServerServiceGeneratorTest {
    @Test
    fun `schema service protocol type exposes every declared protocol`() {
        val model =
            """
            namespace test
            use aws.protocols#awsJson1_0
            use aws.protocols#restJson1
            @awsJson1_0
            @restJson1
            service Example { version: "1" operations: [Ping, BuildInner] }
            @http(method: "POST", uri: "/ping")
            operation Ping { input := {} output := {} }
            @http(method: "POST", uri: "/build-inner")
            operation BuildInner { input := {} output := {} }
            """.asSmithyModel(smithyVersion = "2")

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            testCoverage = HttpTestType.Default,
        ) { context, rustCrate ->
            rustCrate.testModule {
                rustTemplate(
                    """
                    ##[test]
                    fn service_protocol_type_is_a_tuple_of_the_served_protocols() {
                        // Several served protocols: the protocol type is a tuple of them, in served order.
                        fn same<T: 'static>() -> ::std::any::TypeId { ::std::any::TypeId::of::<T>() }
                        assert_eq!(
                            same::<<crate::Example as #{Server}::service::ServiceShape>::Protocol>(),
                            same::<(#{Server}::protocol::aws_json_10::AwsJson1_0, #{Server}::protocol::rest_json_1::RestJson1)>(),
                        );
                        const SCHEMA: &'static #{Server}::schema::ServiceSchema<'static> =
                            <crate::Example as #{Server}::service::SchemaServiceShape>::SCHEMA;
                        let protocols = SCHEMA.protocols();
                        assert!(::std::ptr::eq(SCHEMA, &crate::schema::service::EXAMPLE));
                        let id = <crate::Example as #{Server}::service::ServiceShape>::ID;
                        assert_eq!(id.absolute(), SCHEMA.shape_id().as_str());
                        assert_eq!(id.namespace(), SCHEMA.shape_id().namespace());
                        assert_eq!(id.name(), SCHEMA.shape_id().shape_name());
                        assert_eq!(<crate::Example as #{Server}::service::ServiceShape>::VERSION, SCHEMA.version());
                        assert_eq!(SCHEMA.version(), #{Some}("1"));
                        let mut ids = protocols.iter().map(|id| id.as_str()).collect::<#{Vec}<_>>();
                        ids.sort_unstable();
                        assert_eq!(ids, ["aws.protocols##awsJson1_0", "aws.protocols##restJson1"]);
                    }

                    ##[test]
                    fn missing_handlers_report_schema_operation_ids() {
                        let config = crate::ExampleConfig::builder().build();
                        let error = crate::Example::builder(config).build().err().expect("handlers are missing");
                        let message = error.to_string();
                        assert!(message.contains("test##Ping"));
                        assert!(message.contains("test##BuildInner"));
                        assert!(message.contains(".ping()"));
                        assert!(message.contains(".build_inner()"));
                    }
                    """,
                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    *RuntimeType.preludeScope,
                )
            }
            rustCrate.withFile("tests/service_schema.rs") {
                rustTemplate(
                    """
                    use ${context.moduleUseName()} as sdk;

                    ##[test]
                    fn service_schema_is_public() {
                        let schema = <sdk::Example as #{Server}::service::SchemaServiceShape>::SCHEMA;
                        assert!(::std::ptr::eq(schema, &sdk::schema::service::EXAMPLE));
                        assert!(::std::ptr::eq(
                            <sdk::Example<()> as #{Server}::service::SchemaServiceShape>::SCHEMA,
                            schema,
                        ));
                        assert_eq!(schema.shape_id().as_str(), "test##Example");
                        assert_eq!(schema.operations().len(), 2);
                        let service_id: #{Server}::ShapeId = <sdk::Example as #{Server}::service::ServiceShape>::ID;
                        assert_eq!(service_id.absolute(), schema.shape_id().as_str());
                        type Operations = <sdk::Example as #{Server}::service::ServiceShape>::Operations;
                        let operation_id: #{ShapeId}<'static> = Operations::Ping.shape_id();
                        let typed_operation_id: #{Server}::ShapeId =
                            <sdk::operation_shape::Ping as #{Server}::operation::OperationShape>::ID;
                        assert_eq!(operation_id.as_str(), typed_operation_id.absolute());
                        assert_eq!(
                            &operation_id,
                            <sdk::operation_shape::Ping as #{Server}::operation::SchemaOperationShape>::SCHEMA.shape_id(),
                        );
                    }
                    """,
                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    "ShapeId" to RuntimeType.smithySchema(context.runtimeConfig).resolve("ShapeId"),
                )
            }
        }.forEach { generatedServer ->
            generatedServer.path.resolve("src/schema").toFile().walkTopDown()
                .filter { it.extension == "rs" }
                .forEach { file ->
                    assert(!file.readText().contains("aws_smithy_http_server::shape_id::")) { file.path }
                }
        }
    }

    /**
     * See <https://github.com/smithy-lang/smithy-rs/issues/3177>.
     */
    @Test
    fun `one should be able to return a built service from a function`() {
        val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

        val testDirs =
            serverIntegrationTest(model) { context, rustCrate ->
                rustCrate.testModule {
                    // No actual tests: we just want to check that this compiles.
                    rustTemplate(
                        """
                    fn _build_service() -> crate::SimpleService {
                        let config = crate::SimpleServiceConfig::builder().build();
                        let service = crate::SimpleService::builder(config).build_unchecked();

                        service.boxed()
                    }

                    fn _protocol_is_rest_json(
                        protocol: <crate::SimpleService as #{Server}::service::ServiceShape>::Protocol,
                    ) -> #{Server}::protocol::rest_json_1::RestJson1 {
                        protocol
                    }
                    """,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    )
                }
            }

        // test the generated metadata for all generated projects (both HTTP 0.x and HTTP 1.x)
        testDirs.forEach { generatedServer ->
            val cargoToml = generatedServer.path.resolve("Cargo.toml").readText()
            assert(cargoToml.contains("codegen-version =")) { cargoToml }
            assert(cargoToml.contains("protocol = \"aws.protocols#restJson1\"")) { cargoToml }
            val service = generatedServer.path.resolve("src/service.rs").readText()
            assert(!service.contains("SchemaServiceShape")) { service }
        }
    }

    /**
     * The generated serde builder must not require handlers, HTTP plugins or layers to be `Sync`:
     * only the schema router shares its routes across threads.
     */
    @Test
    fun `legacy handlers and http plugins need not be Sync`() {
        val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":false}}""").expectObjectNode(),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.testModule {
                // No actual tests: we just want to check that this compiles.
                rustTemplate(
                    """
                    /// Wraps a service so it is `Send + Clone` but not `Sync` (`Cell<()>` is `!Sync`).
                    ##[allow(dead_code)]
                    ##[derive(Clone)]
                    struct NotSync<S> {
                        inner: S,
                        _not_sync: ::std::cell::Cell<()>,
                    }
                    impl<S: #{Tower}::Service<R>, R> #{Tower}::Service<R> for NotSync<S> {
                        type Response = S::Response;
                        type Error = S::Error;
                        type Future = S::Future;
                        fn poll_ready(&mut self, cx: &mut ::std::task::Context<'_>) -> ::std::task::Poll<#{Result}<(), Self::Error>> {
                            self.inner.poll_ready(cx)
                        }
                        fn call(&mut self, request: R) -> Self::Future {
                            self.inner.call(request)
                        }
                    }
                    ##[allow(dead_code)]
                    struct NotSyncLayer;
                    impl<S> #{Tower}::Layer<S> for NotSyncLayer {
                        type Service = NotSync<S>;
                        fn layer(&self, inner: S) -> Self::Service {
                            NotSync { inner, _not_sync: ::std::cell::Cell::new(()) }
                        }
                    }

                    fn _build_service() {
                        let plugin = #{Server}::plugin::LayerPlugin(NotSyncLayer);
                        let config = crate::SimpleServiceConfig::builder().http_plugin(plugin).build();
                        // A `Cell` is `Send + Clone` but not `Sync`.
                        let hits = ::std::cell::Cell::new(0u32);
                        // Pins the request body to the router's default, as a real server does.
                        let _app: crate::SimpleService<
                            #{Server}::routing::RoutingService<
                                #{Server}::protocol::rest::router::RestRouter<#{Server}::routing::Route>,
                                #{Server}::protocol::rest_json_1::RestJson1,
                            >,
                        > = crate::SimpleService::builder(config)
                            .operation(move |_input: crate::input::OperationInput| {
                                let hits = hits.clone();
                                async move {
                                    hits.set(hits.get() + 1);
                                    crate::output::OperationOutput::builder().build()
                                }
                            })
                            .build_unchecked();
                    }
                    """,
                    "Server" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                    "Tower" to ServerCargoDependency.tower(codegenContext).toType(),
                    *RuntimeType.preludeScope,
                )
            }
        }
    }
}
