/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerProtocolMap
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerHttpBoundProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerRestJsonFactory
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class SchemaRoutingGeneratorTest {
    @Test
    fun `schema builders never invoke protocol routing codegen and layers run after selection`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1
            @restJson1
            service Example { operations: [Ping, Other] }
            @http(method: "POST", uri: "/ping")
            operation Ping { input := { message: String } output := {} }
            @http(method: "POST", uri: "/other")
            operation Other { input := {} output := {} }
            """.asSmithyModel(smithyVersion = "2")

        fun schemaProtocol(context: ServerCodegenContext): ServerProtocol =
            object : ServerProtocol by ServerRestJsonProtocol(context) {
                override fun routerType(): RuntimeType = error("schema generation invoked routerType")

                override fun serverRouterRuntimeConstructor(): String =
                    error("schema generation invoked router constructor")

                override fun serverRouterRequestSpec(
                    operationShape: OperationShape,
                    operationName: String,
                    serviceName: String,
                    requestSpecModule: RuntimeType,
                ): Writable = error("schema generation invoked request spec")

                override fun serverRouterRequestSpecType(requestSpecModule: RuntimeType): RuntimeType =
                    error("schema generation invoked request spec type")
            }
        val decorator =
            object : ServerCodegenDecorator {
                override val name = "Schema routing test"
                override val order: Byte = 0

                override fun protocols(
                    serviceId: ShapeId,
                    currentProtocols: ServerProtocolMap,
                ): ServerProtocolMap =
                    currentProtocols + (
                        ShapeId.from("aws.protocols#restJson1") to
                            object : ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext> {
                                override fun protocol(codegenContext: ServerCodegenContext) =
                                    schemaProtocol(codegenContext)

                                override fun buildProtocolGenerator(codegenContext: ServerCodegenContext) =
                                    ServerHttpBoundProtocolGenerator(codegenContext, schemaProtocol(codegenContext))

                                override fun support() = ServerRestJsonFactory().support()
                            }
                    )
            }
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = ObjectNode.builder().withMember("codegen", ObjectNode.builder().withMember("schemaSerde", true).build()).build()),
            additionalDecorators = listOf(decorator),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            val scope =
                arrayOf(
                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    "Http" to RuntimeType.http(context.runtimeConfig),
                    "Tower" to ServerCargoDependency.Tower.toType(),
                    *RuntimeType.preludeScope,
                )
            crate.testModule {
                tokioTest("body_configuration_is_final_at_build_and_custom_handlers_remain_usable") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        for before in [false, true] {
                            for allowed in [false, true] {
                                let body_config = #{Server}::schema::ServiceRequestBodyConfig {
                                    global: #{Server}::schema::RequestBodyCollectionConfig {
                                        max_bytes: ::std::num::NonZeroUsize::new(1), read_timeout: #{None},
                                    },
                                    per_operation: if allowed {
                                        [("test##Ping".to_owned(), #{Server}::schema::RequestBodyCollectionConfig::default())].into()
                                    } else { ::std::collections::HashMap::new() },
                                };
                                let builder = crate::Example::builder(crate::ExampleConfig::builder().build());
                                let builder = if before { builder.request_body_config(body_config.clone()) } else { builder };
                                let builder = builder.ping(|input: crate::input::PingInput| async move {
                                    assert_eq!(input.message.as_deref(), #{Some}("hello"));
                                    crate::output::PingOutput {}
                                });
                                let builder = if before { builder } else { builder.request_body_config(body_config) };
                                let service = builder.build_unchecked();
                                let request = #{Http}::Request::builder().method("POST").uri("/ping").header("content-type", "application/json")
                                    .body(#{Server}::body::Body::from_bytes(r##"{"message":"hello"}"##.into())).unwrap();
                                let response = service.oneshot(request).await.unwrap();
                                assert_eq!(response.status(), if allowed { 200 } else { 400 });
                            }
                        }
                        let custom = #{Tower}::service_fn(|request: #{Http}::Request<#{Server}::body::Body>| async move {
                            assert!(request.extensions().get::<#{Server}::schema::SelectedProtocolOperation>().is_some());
                            #{Ok}::<_, ::std::convert::Infallible>(#{Http}::Response::builder().status(202).body(#{Server}::body::empty()).unwrap())
                        });
                        let service = crate::Example::builder(crate::ExampleConfig::builder().build()).ping_custom(custom).build_unchecked();
                        let request = #{Http}::Request::builder().method("POST").uri("/ping").body(#{Server}::body::Body::empty()).unwrap();
                        assert_eq!(service.oneshot(request).await.unwrap().status(), 202);
                        """,
                        *scope,
                    )
                }
                tokioTest("http_plugin_wraps_the_configured_upgrade_and_replaced_body_is_deserialized") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        let layer = #{Tower}::layer::layer_fn(|inner| {
                            #{Tower}::ServiceBuilder::new().map_request(|mut request: #{Http}::Request<#{Server}::body::Body>| {
                                assert!(request.extensions().get::<#{Server}::schema::SelectedProtocolOperation>().is_some());
                                *request.body_mut() = #{Server}::body::Body::from_bytes(r##"{"message":"replacement"}"##.into());
                                request
                            }).service(inner)
                        });
                        let config = crate::ExampleConfig::builder().http_plugin(#{Server}::plugin::LayerPlugin(layer)).build();
                        let service = crate::Example::builder(config).ping_service(#{Tower}::service_fn(|input: crate::input::PingInput| async move {
                            assert_eq!(input.message.as_deref(), #{Some}("replacement"));
                            #{Ok}::<_, ::std::convert::Infallible>(crate::output::PingOutput {})
                        })).request_body_config(#{Server}::schema::ServiceRequestBodyConfig::default()).build_unchecked();
                        let request = #{Http}::Request::builder().method("POST").uri("/ping").header("content-type", "application/json")
                            .body(#{Server}::body::Body::from_bytes("invalid original json".into())).unwrap();
                        assert_eq!(service.oneshot(request).await.unwrap().status(), 200);
                        """,
                        *scope,
                    )
                }
                tokioTest("checked_and_unchecked_builders_preserve_handlers_and_layer_selection") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        let config = crate::ExampleConfig::builder().build();
                        assert!(matches!(crate::Example::builder(config).build(), #{Err}(crate::BuildError::MissingOperations(_))));
                        let calls = ::std::sync::Arc::new(::std::sync::atomic::AtomicUsize::new(0));
                        for checked in [false, true] {
                            let calls = calls.clone();
                            let layer = #{Tower}::layer::layer_fn(move |inner: #{Server}::routing::Route<#{Server}::body::Body>| {
                                let calls = calls.clone();
                                #{Tower}::service_fn(move |request: #{Http}::Request<#{Server}::body::Body>| {
                                    assert!(request.extensions().get::<#{Server}::schema::SelectedProtocolOperation>().is_some());
                                    calls.fetch_add(1, ::std::sync::atomic::Ordering::SeqCst);
                                    inner.clone().oneshot(request)
                                })
                            });
                            let config = crate::ExampleConfig::builder().layer(layer).build();
                            let builder = crate::Example::builder(config).ping(|_input| async { crate::output::PingOutput {} });
                            let service = if checked {
                                builder.other(|_input| async { crate::output::OtherOutput {} }).build().unwrap()
                            } else { builder.build_unchecked() };
                            let request = |path| #{Http}::Request::builder().method("POST").uri(path).body(#{Server}::body::Body::empty()).unwrap();
                            assert_eq!(service.clone().oneshot(request("/ping")).await.unwrap().status(), 200);
                            assert_eq!(service.clone().oneshot(request("/other")).await.unwrap().status(), if checked { 200 } else { 500 });
                            assert_eq!(service.oneshot(request("/unknown")).await.unwrap().status(), 404);
                        }
                        assert_eq!(calls.load(::std::sync::atomic::Ordering::SeqCst), 4);
                        """,
                        *scope,
                    )
                }
            }
        }
    }

    @Test
    fun `both builders reject body routing for generated streaming operations`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1
            @restJson1
            service Example { operations: [Stream] }
            @http(method: "POST", uri: "/stream")
            operation Stream {
                input := { @required @httpPayload data: Data }
                output := {}
            }
            @streaming
            blob Data
            """.asSmithyModel(smithyVersion = "2")
        for (bodyRouting in listOf(false, true)) {
            val decorator =
                object : ServerCodegenDecorator {
                    override val name = "Body routing streaming validation"
                    override val order: Byte = 0

                    override fun additionalProtocolRegistrations(codegenContext: ServerCodegenContext): List<Writable> =
                        listOf(
                            {
                                rustTemplate(
                                    """
                                    {
                                        use #{Server}::schema::{ServerProtocol, SharedServerProtocol, ProtocolRegistration};
                                        use #{Server}::routing::{ProtocolRouter, SharedProtocolRouter, OperationIndex, RouterBuildError, SchemaRoutingOptions};
                                        use #{Server}::body::{Body, BoxBody};
                                        use #{Schema}::{Schema, ShapeId, ServiceSchema};
                                        use #{Http}::{Request, Response};
                                        ##[derive(Debug)]
                                        struct TestRouter(SharedProtocolRouter);
                                        impl ProtocolRouter for TestRouter {
                                            fn requires_body_for_routing(&self) -> bool { $bodyRouting }
                                            fn route(&self, request: &Request<Body>, bytes: &#{Bytes}) -> #{Result}<OperationIndex, Response<BoxBody>> {
                                                self.0.route(request, bytes)
                                            }
                                        }
                                        ##[derive(Debug)]
                                        struct TestProtocol;
                                        impl ServerProtocol for TestProtocol {
                                            fn protocol_id(&self) -> &'static ShapeId<'static> {
                                                static ID: ShapeId<'static> = #{Schema}::shape_id!("test", "bodyRouting");
                                                &ID
                                            }
                                            fn build_router(&self, service: &'static ServiceSchema<'static>, targets: &[OperationIndex], options: &SchemaRoutingOptions)
                                                -> #{Result}<SharedProtocolRouter, RouterBuildError> {
                                                let router = #{Server}::protocol::rest_json_1::RestJson1Protocol::default().build_router(service, targets, options)?;
                                                #{Ok}(SharedProtocolRouter::new(TestRouter(router)))
                                            }
                                            fn deserialize_request<'a>(&'a self, _: &Schema<'_>, _: &'a #{Server}::schema::ServerRequest)
                                                -> #{Result}<#{Box}<dyn #{Schema}::serde::ShapeDeserializer + 'a>, #{Server}::schema::DeserializeError> { unreachable!() }
                                            fn serialize_response(&self, _: &Schema<'_>, _: &dyn #{Schema}::serde::SerializableStruct) -> Response<BoxBody> { unreachable!() }
                                            fn serialize_streaming_response(&self, _: &Schema<'_>, _: &dyn #{Schema}::serde::SerializableStruct, _: BoxBody) -> Response<BoxBody> { unreachable!() }
                                            fn serialize_error(&self, _: &dyn #{Server}::schema::HttpModeledError) -> Response<BoxBody> { unreachable!() }
                                            fn serialize_rejection(&self, _: #{Server}::schema::DeserializeError) -> Response<BoxBody> { unreachable!() }
                                            fn serialize_internal_failure(&self) -> Response<BoxBody> { unreachable!() }
                                        }
                                        ProtocolRegistration::new(|_| #{Some}(SharedServerProtocol::new(TestProtocol)))
                                    }
                                    """,
                                    "Server" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                                    "Schema" to RuntimeType.smithySchema(codegenContext.runtimeConfig),
                                    "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                                    "Bytes" to RuntimeType.Bytes,
                                    *RuntimeType.preludeScope,
                                )
                            },
                        )
                }
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = ObjectNode.builder().withMember("codegen", ObjectNode.builder().withMember("schemaSerde", true).build()).build()),
                additionalDecorators = listOf(decorator),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("streaming_validation_preserves_checked_errors_and_unchecked_panics") {
                        rustTemplate(
                            """
                            let builder = || crate::Example::builder(crate::ExampleConfig::builder().build());
                            let unchecked = ::std::panic::catch_unwind(|| {
                                let _: crate::Example = builder().build_unchecked();
                            });
                            let checked = builder().stream_custom(#{Tower}::service_fn(|_: #{Http}::Request<#{Server}::body::Body>| async {
                                #{Ok}::<_, ::std::convert::Infallible>(#{Http}::Response::new(#{Server}::body::empty()))
                            })).build();
                            if $bodyRouting {
                                let panic = unchecked.expect_err("unchecked construction must panic");
                                let message = panic.downcast_ref::<#{String}>().expect("panic message");
                                assert!(message.contains("invalid schema routing configuration"));
                                assert!(message.contains("test##bodyRouting"));
                                assert!(message.contains("test##Stream"));
                                assert!(matches!(checked, #{Err}(crate::BuildError::Routing(#{Server}::routing::RouterBuildError::StreamingBodyRouting { protocol, operation }))
                                    if protocol == "test##bodyRouting" && operation == "test##Stream"));
                            } else {
                                assert!(unchecked.is_ok());
                                assert!(checked.is_ok());
                            }
                            """,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.Tower.toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
    }
}
