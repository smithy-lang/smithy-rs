/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
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

/**
 * Exercises generated schema service builders and routing with ordinary, streaming, and custom
 * protocol operations. It checks protocol selection, missing handlers, middleware ordering and
 * rebuilt requests, propagation of body limits and protocol flags, and XML collection-name
 * settings. Decorator fixtures also verify that schema builders use runtime protocol registries
 * and that legacy generation does not request those registries.
 */
class SchemaRoutingGeneratorTest {
    @Test
    fun `missing handlers use selected protocols and tolerate rebuilt requests`() {
        for (customProtocol in listOf(false, true)) {
            val protocols =
                if (customProtocol) {
                    """
                    @protocolDefinition
                    @trait(selector: "service")
                    structure bodyRouting {}
                    @bodyRouting
                    """
                } else {
                    """
                    use aws.protocols#restJson1
                    use smithy.protocols#rpcv2Cbor
                    @restJson1
                    @rpcv2Cbor
                    """
                }
            val model =
                """
                namespace test
                $protocols
                service Example { operations: [Ping] }
                @http(method: "POST", uri: "/ping")
                operation Ping { input := {} output := {} }
                """.asSmithyModel(smithyVersion = "2")
            val decorator =
                object : ServerCodegenDecorator {
                    override val name = "Missing handler custom protocol"
                    override val order: Byte = 0

                    override fun protocolRegistries(codegenContext: ServerCodegenContext): List<RuntimeType> =
                        if (customProtocol) {
                            listOf(testProtocolRegistry(codegenContext, false))
                        } else {
                            emptyList()
                        }

                    override fun protocols(
                        serviceId: ShapeId,
                        currentProtocols: ServerProtocolMap,
                    ): ServerProtocolMap =
                        if (customProtocol) {
                            currentProtocols + (
                                ShapeId.from("test#bodyRouting") to
                                    object : ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext> {
                                        override fun protocol(codegenContext: ServerCodegenContext) =
                                            ServerRestJsonProtocol(codegenContext)

                                        override fun buildProtocolGenerator(codegenContext: ServerCodegenContext) =
                                            ServerHttpBoundProtocolGenerator(codegenContext, ServerRestJsonProtocol(codegenContext))

                                        override fun support() = ServerRestJsonFactory().support()
                                    }
                            )
                        } else {
                            currentProtocols
                        }
                }
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                additionalDecorators = listOf(decorator),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("missing_handler_fallback_survives_context_loss") {
                        val requests =
                            if (customProtocol) {
                                """[("/ping", "application/json", b"{}".as_slice())]"""
                            } else {
                                """[
                                    ("/ping", "application/json", b"{}".as_slice()),
                                    ("/service/Example/operation/Ping", "application/cbor", b"\xa0".as_slice()),
                                ]"""
                            }
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            for remove_context in [false, true] {
                                let layer = #{Tower}::ServiceBuilder::new().map_request(move |request: #{Http}::Request<#{Server}::body::Body>| {
                                    assert!(#{Server}::schema::SelectedOperation::get_from_request(&request).is_some());
                                    if remove_context {
                                        let (mut parts, body) = request.into_parts();
                                        parts.extensions = #{Default}::default();
                                        #{Http}::Request::from_parts(parts, body)
                                    } else { request }
                                });
                                let service = crate::Example::builder(crate::ExampleConfig::builder().layer(layer).build()).build_unchecked();
                                for (uri, content_type, expected_body) in $requests {
                                    let request = #{Http}::Request::builder().method("POST").uri(uri)
                                        .header("content-type", content_type)
                                        .header("smithy-protocol", "rpc-v2-cbor")
                                        .body(#{Server}::body::Body::empty()).unwrap();
                                    let response = service.clone().oneshot(request).await.unwrap();
                                    assert_eq!(response.status(), 500);
                                    if remove_context {
                                        assert!(response.headers().is_empty());
                                    } else {
                                        assert_eq!(response.headers()["content-type"], content_type);
                                        if $customProtocol {
                                            assert_eq!(response.headers()["x-test-protocol"], "0");
                                        }
                                    }
                                    let body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                                    assert_eq!(body.as_ref(), if remove_context { b"".as_slice() } else { expected_body });
                                }
                            }
                            """,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `adding RPC protocols preserves headerless REST requests and defers required body rejections`() {
        for ((extraProtocol, extraTrait) in listOf(
            "" to "",
            "use smithy.protocols#rpcv2Cbor" to "@rpcv2Cbor",
            "use aws.protocols#awsJson1_1" to "@awsJson1_1",
        )) {
            val model =
                """
                namespace test
                use aws.protocols#restJson1
                $extraProtocol
                @restJson1
                $extraTrait
                service Example { operations: [Read, Remove, OptionalBody, RequiredBody, DefaultBody, Empty] }
                @http(method: "GET", uri: "/items/{id}")
                operation Read { input := { @required @httpLabel id: String } output := {} }
                @http(method: "DELETE", uri: "/items/{id}")
                operation Remove { input := { @required @httpLabel id: String } output := {} }
                @http(method: "POST", uri: "/optional")
                operation OptionalBody { input := { value: String } output := {} }
                @http(method: "POST", uri: "/required")
                operation RequiredBody { input := { @required value: String } output := {} }
                @http(method: "POST", uri: "/default")
                operation DefaultBody { input := { @required @default("fallback") value: String } output := {} }
                @http(method: "POST", uri: "/empty")
                operation Empty { input := {} output := {} }
                """.asSmithyModel(smithyVersion = "2")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("headerless_requests_reach_the_existing_handler") {
                        val handlers =
                            listOf(
                                Triple("Read", "read", """assert_eq!(input.id, "123");"""),
                                Triple("Remove", "remove", """assert_eq!(input.id, "123");"""),
                                Triple("OptionalBody", "optional_body", "assert!(input.value.is_none());"),
                                Triple("RequiredBody", "required_body", """assert_eq!(input.value, "present");"""),
                                Triple("DefaultBody", "default_body", """assert_eq!(input.value, "fallback");"""),
                                Triple("Empty", "empty", "let _ = input;"),
                            ).joinToString("\n") { (operation, method, assertion) ->
                                val operationShape = context.model.expectShape(ShapeId.from("test#$operation"), OperationShape::class.java)
                                val output = "crate::output::${operation}Output {}"
                                val result =
                                    if (operationShape.errors.isEmpty()) {
                                        output
                                    } else {
                                        "#{Ok}($output)"
                                    }
                                """
                                .$method(move |input: crate::input::${operation}Input| async move {
                                    $assertion
                                    $result
                                })
                                """
                            }
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            let service = crate::Example::builder(crate::ExampleConfig::builder().build())
                                $handlers
                                .build_unchecked();
                            let request = |method: &str, uri: &str, body: &str, content_type: #{Option}<&str>| {
                                let mut builder = #{Http}::Request::builder().method(method).uri(uri);
                                if let #{Some}(content_type) = content_type {
                                    builder = builder.header("content-type", content_type);
                                }
                                builder.body(#{Server}::body::Body::from_bytes(#{Bytes}::copy_from_slice(body.as_bytes()))).unwrap()
                            };
                            // These exact requests are replayed for the sole REST service and both RPC additions.
                            for (method, uri) in [
                                ("GET", "/items/123"), ("DELETE", "/items/123"),
                                ("POST", "/optional"), ("POST", "/default"), ("POST", "/empty"),
                            ] {
                                let response = service.clone().oneshot(request(method, uri, "", #{None})).await.unwrap();
                                assert_eq!(response.status(), 200, "{method} {uri}");
                            }
                            let response = service.clone().oneshot(request("POST", "/optional", r##"{"value":"present"}"##, #{None})).await.unwrap();
                            assert_eq!(response.status(), 415);
                            for body in ["", "{}", r##"{"value":"present"}"##] {
                                let response = service.clone().oneshot(request("POST", "/required", body, #{None})).await.unwrap();
                                // A sole REST protocol retains generated serde zero-byte required-field validation.
                                let expected = if ${extraTrait.isEmpty()} && body.is_empty() { 400 } else { 415 };
                                assert_eq!(response.status(), expected);
                            }
                            let response = service.clone().oneshot(request("POST", "/required", r##"{"value":"present"}"##, #{Some}("application/json"))).await.unwrap();
                            assert_eq!(response.status(), 200);
                            """,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Bytes" to RuntimeType.Bytes,
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
    }

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
            IntegrationTestParams(additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode()),
            additionalDecorators = listOf(decorator),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            val scope =
                arrayOf(
                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    "Http" to RuntimeType.http(context.runtimeConfig),
                    "Tower" to ServerCargoDependency.tower(context).toType(),
                    *RuntimeType.preludeScope,
                )
            crate.withModule(RustModule.inlineTests("schema_routing_tests", parent = ServerRustModule.Service)) {
                tokioTest("body_configuration_is_final_at_build_and_custom_handlers_remain_usable") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        for before in [false, true] {
                            for allowed in [false, true] {
                                let body_config = #{Server}::config::ServiceRequestBodyConfig::default()
                                    .with_global(#{Server}::config::RequestBodyCollectionConfig::default()
                                        .with_max_bytes(::std::num::NonZeroUsize::new(1)))
                                    .with_per_operation(if allowed {
                                        [("test##Ping".to_owned(), #{Server}::config::RequestBodyCollectionConfig::default())].into()
                                    } else { ::std::collections::HashMap::new() });
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
                            assert!(#{Server}::schema::SelectedOperation::get_from_request(&request).is_some());
                            #{Ok}::<_, ::std::convert::Infallible>(#{Http}::Response::builder().status(202).body(#{Server}::body::empty()).unwrap())
                        });
                        let service = crate::Example::builder(crate::ExampleConfig::builder().build()).ping_custom(custom).build_unchecked();
                        let request = #{Http}::Request::builder().method("POST").uri("/ping").header("content-type", "application/json").body(#{Server}::body::Body::empty()).unwrap();
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
                                assert!(#{Server}::schema::SelectedOperation::get_from_request(&request).is_some());
                                *request.body_mut() = #{Server}::body::Body::from_bytes(r##"{"message":"replacement"}"##.into());
                                request
                            }).service(inner)
                        });
                        let config = crate::ExampleConfig::builder().http_plugin(#{Server}::plugin::LayerPlugin(layer)).build();
                        let service = crate::Example::builder(config).ping_service(#{Tower}::service_fn(|input: crate::input::PingInput| async move {
                            assert_eq!(input.message.as_deref(), #{Some}("replacement"));
                            #{Ok}::<_, ::std::convert::Infallible>(crate::output::PingOutput {})
                        })).request_body_config(#{Server}::config::ServiceRequestBodyConfig::default()).build_unchecked();
                        let request = #{Http}::Request::builder().method("POST").uri("/ping").header("content-type", "application/json")
                            .body(#{Server}::body::Body::from_bytes("invalid original json".into())).unwrap();
                        assert_eq!(service.oneshot(request).await.unwrap().status(), 200);
                        """,
                        *scope,
                    )
                }
                tokioTest("http_plugin_body_replacement_preserves_the_routed_body_limit") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        let layer = #{Tower}::layer::layer_fn(|inner| {
                            #{Tower}::ServiceBuilder::new().map_request(|mut request: #{Http}::Request<#{Server}::body::Body>| {
                                let selected = #{Server}::schema::SelectedOperation::get_from_request(&request).unwrap();
                                assert_eq!(selected.request_body_config().max_bytes, ::std::num::NonZeroUsize::new(1));
                                *request.body_mut() = #{Server}::body::Body::from_bytes(r##"{"message":"replacement"}"##.into());
                                request
                            }).service(inner)
                        });
                        let limit = #{Server}::config::ServiceRequestBodyConfig::default()
                            .with_global(#{Server}::config::RequestBodyCollectionConfig::default()
                                .with_max_bytes(::std::num::NonZeroUsize::new(1)));
                        let config = crate::ExampleConfig::builder().http_plugin(#{Server}::plugin::LayerPlugin(layer)).build();
                        let service = crate::Example::builder(config)
                            // A request that reached the handler would get 200.
                            .ping(|_input: crate::input::PingInput| async { crate::output::PingOutput {} })
                            .request_body_config(limit)
                            .build_unchecked();
                        let request = #{Http}::Request::builder().method("POST").uri("/ping").header("content-type", "application/json")
                            .body(#{Server}::body::Body::empty()).unwrap();
                        assert_eq!(service.oneshot(request).await.unwrap().status(), 400);
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
                            let layer = #{Tower}::layer::layer_fn(move |inner: #{Server}::routing::SyncRoute<#{Server}::body::Body>| {
                                let calls = calls.clone();
                                #{Tower}::service_fn(move |request: #{Http}::Request<#{Server}::body::Body>| {
                                    assert!(#{Server}::schema::SelectedOperation::get_from_request(&request).is_some());
                                    calls.fetch_add(1, ::std::sync::atomic::Ordering::SeqCst);
                                    inner.clone().oneshot(request)
                                })
                            });
                            let config = crate::ExampleConfig::builder().layer(layer).build();
                            let builder = crate::Example::builder(config).ping(|_input| async { crate::output::PingOutput {} });
                            let service = if checked {
                                builder.other(|_input| async { crate::output::OtherOutput {} }).build().unwrap()
                            } else { builder.build_unchecked() };
                            let request = |path| #{Http}::Request::builder().method("POST").uri(path).header("content-type", "application/json").body(#{Server}::body::Body::empty()).unwrap();
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
    fun `both builders accept body routing alongside generated streaming operations`() {
        val model =
            """
            namespace test
            @protocolDefinition
            @trait(selector: "service")
            structure bodyRouting {}
            @bodyRouting
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

                    override fun protocolRegistries(codegenContext: ServerCodegenContext): List<RuntimeType> =
                        listOf(testProtocolRegistry(codegenContext, bodyRouting))

                    override fun protocols(
                        serviceId: ShapeId,
                        currentProtocols: ServerProtocolMap,
                    ): ServerProtocolMap =
                        currentProtocols + (
                            ShapeId.from("test#bodyRouting") to
                                object : ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext> {
                                    override fun protocol(codegenContext: ServerCodegenContext) =
                                        ServerRestJsonProtocol(codegenContext)

                                    override fun buildProtocolGenerator(codegenContext: ServerCodegenContext) =
                                        ServerHttpBoundProtocolGenerator(codegenContext, ServerRestJsonProtocol(codegenContext))

                                    override fun support() = ServerRestJsonFactory().support()
                                }
                        )
                }
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode()),
                additionalDecorators = listOf(decorator),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.withModule(RustModule.inlineTests("streaming_routing_tests", parent = ServerRustModule.Service)) {
                    tokioTest("streaming_operations_do_not_fail_either_builder") {
                        rustTemplate(
                            """
                            let builder = || crate::Example::builder(crate::ExampleConfig::builder().build());
                            let unchecked = ::std::panic::catch_unwind(|| {
                                let _: crate::Example = builder().build_unchecked();
                            });
                            let checked = builder().stream_custom(#{Tower}::service_fn(|_: #{Http}::Request<#{Server}::body::Body>| async {
                                #{Ok}::<_, ::std::convert::Infallible>(#{Http}::Response::new(#{Server}::body::empty()))
                            })).build();
                            // A body-first protocol leaves streaming operations out of its routing table
                            // rather than failing the build.
                            assert!(unchecked.is_ok());
                            assert!(checked.is_ok());
                            """,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `one service decorator supplies five runtime protocols through one registry`() {
        val model =
            """
            namespace test
            @protocolDefinition
            @trait(selector: "service")
            structure first {}
            @protocolDefinition
            @trait(selector: "service")
            structure second {}
            @protocolDefinition
            @trait(selector: "service")
            structure third {}
            @protocolDefinition
            @trait(selector: "service")
            structure fourth {}
            @protocolDefinition
            @trait(selector: "service")
            structure unused {}
            @first
            @second
            service Example { operations: [Ping] }
            @http(method: "POST", uri: "/ping")
            operation Ping { input := {} output := {} }
            """.asSmithyModel(smithyVersion = "2")
        var calls = 0
        val protocolIds = listOf("test#first", "test#second", "test#third", "test#fourth", "test#unused")
        val decorator =
            object : ServerCodegenDecorator {
                override val name = "Shared protocol registry"
                override val order: Byte = 0

                override fun protocolRegistries(codegenContext: ServerCodegenContext): List<RuntimeType> {
                    codegenContext.servedProtocols shouldBe listOf(ShapeId.from("test#first"), ShapeId.from("test#second"))
                    calls++
                    return listOf(testProtocolRegistry(codegenContext, true, protocolIds))
                }

                override fun protocols(
                    serviceId: ShapeId,
                    currentProtocols: ServerProtocolMap,
                ): ServerProtocolMap =
                    currentProtocols +
                        protocolIds.associate { id ->
                            ShapeId.from(id) to
                                object : ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext> {
                                    override fun protocol(codegenContext: ServerCodegenContext) =
                                        ServerRestJsonProtocol(codegenContext)

                                    override fun buildProtocolGenerator(codegenContext: ServerCodegenContext) =
                                        ServerHttpBoundProtocolGenerator(codegenContext, ServerRestJsonProtocol(codegenContext))

                                    override fun support() = ServerRestJsonFactory().support()
                                }
                        }
            }
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde().toObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            additionalDecorators = listOf(decorator),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.withModule(RustModule.inlineTests("shared_registry_tests", parent = ServerRustModule.Service)) {
                tokioTest("a_shared_registry_builds_without_duplicate_protocol_registrations") {
                    rustTemplate(
                        """
                        // One registry contains all five protocols. Only the two declared
                        // by the service are constructed, and both builders must succeed.
                        let builder = || crate::Example::builder(crate::ExampleConfig::builder().build());
                        let _: crate::Example = builder().build_unchecked();
                        let checked = builder().ping_custom(#{Tower}::service_fn(|_: #{Http}::Request<#{Server}::body::Body>| async {
                            #{Ok}::<_, ::std::convert::Infallible>(#{Http}::Response::new(#{Server}::body::empty()))
                        })).build();
                        assert!(checked.is_ok());
                        """,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        *RuntimeType.preludeScope,
                    )
                }
            }
        }
        calls shouldBe 1
    }

    @Test
    fun `legacy codegen never requests schema protocol registries`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1
            @restJson1
            service Example { operations: [Ping] }
            @http(method: "POST", uri: "/ping")
            operation Ping { input := {} output := {} }
            """.asSmithyModel(smithyVersion = "2")
        val decorator =
            object : ServerCodegenDecorator {
                override val name = "Legacy registry guard"
                override val order: Byte = 0

                override fun protocolRegistries(codegenContext: ServerCodegenContext): List<RuntimeType> =
                    error("Legacy codegen requested a schema protocol registry")
            }
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(false).toObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            additionalDecorators = listOf(decorator),
            testCoverage = HttpTestType.ALL,
        )
    }

    @Test
    fun `schema CBOR defaults to 405 and supports Java method mismatch responses`() {
        val model =
            """
            namespace test
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor
            service Example { operations: [Ping] }
            operation Ping { input := {} output := {} }
            """.asSmithyModel(smithyVersion = "2")
        for (asNotFound in listOf(null, false, true)) {
            val settings =
                ServerAdditionalSettings.builder().schemaSerde().toObjectNode().toBuilder()
            if (asNotFound != null) {
                settings.withMember(
                    "customizationConfig",
                    ObjectNode.builder().withMember(
                        "protocols",
                        ObjectNode.builder().withMember(
                            "smithy.protocols#rpcv2Cbor",
                            ObjectNode.builder().withMember("methodNotAllowedAsNotFound", asNotFound).build(),
                        ).build(),
                    ).build(),
                )
            }
            val expectedStatus =
                if (asNotFound == true) {
                    404
                } else {
                    405
                }
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = settings.build()),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("get_returns_configured_status_and_post_dispatches") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            let service = crate::Example::builder(crate::ExampleConfig::builder().build())
                                .ping(|_input: crate::input::PingInput| async { crate::output::PingOutput {} })
                                .build()
                                .unwrap();
                            for (method, expected) in [("GET", $expectedStatus), ("POST", 200)] {
                                let request = #{Http}::Request::builder()
                                    .method(method)
                                    .uri("/service/Example/operation/Ping")
                                    .header("content-type", "application/cbor")
                                    .header("smithy-protocol", "rpc-v2-cbor")
                                    .body(#{Server}::body::Body::from_bytes(vec![0xA0u8].into()))
                                    .unwrap();
                                let response = service.clone().oneshot(request).await.unwrap();
                                assert_eq!(response.status(), expected, "{method}");
                            }
                            """,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
    }

    @Test
    fun `customizationConfig protocols sections reach the schema router`() {
        val model =
            """
            namespace test
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor
            service Example { operations: [getFoo] }
            operation getFoo { input := { value: String } output := {} }
            """.asSmithyModel(smithyVersion = "2")
        val settings =
            ServerAdditionalSettings.builder().schemaSerde().toObjectNode().toBuilder()
                .withMember(
                    "customizationConfig",
                    ObjectNode.builder().withMember(
                        "protocols",
                        ObjectNode.builder()
                            .withMember(
                                "smithy.protocols#rpcv2Cbor",
                                ObjectNode.builder().withMember("capitalizeRoutes", true).build(),
                            )
                            // Codegen translates this into typed body configuration for every protocol.
                            .withMember("common", Node.parse("""{"request":{"body":{"maxBytes":64}}}""").expectObjectNode())
                            // An unrelated protocol's section passes through without being consulted.
                            .withMember(
                                "example.protocols#customRpc",
                                ObjectNode.builder().withMember("anything", "opaque").build(),
                            )
                            .build(),
                    ).build(),
                )
                .build()
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = settings),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("capitalized_alias_and_verbatim_route_both_succeed") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        let service = crate::Example::builder(crate::ExampleConfig::builder().build())
                            .get_foo(|_input: crate::input::GetFooInput| async { crate::output::GetFooOutput {} })
                            .build()
                            .unwrap();
                        for path in ["/service/Example/operation/getFoo", "/service/Example/operation/GetFoo"] {
                            let request = #{Http}::Request::builder()
                                .method("POST")
                                .uri(path)
                                .header("content-type", "application/cbor")
                                .header("smithy-protocol", "rpc-v2-cbor")
                                // An empty CBOR map: every input member is optional.
                                .body(#{Server}::body::Body::from_bytes(vec![0xA0u8].into()))
                                .unwrap();
                            let response = service.clone().oneshot(request).await.unwrap();
                            assert_eq!(response.status(), 200, "{path}");
                        }
                        """,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        *RuntimeType.preludeScope,
                    )
                }
                tokioTest("global_request_body_limit_is_applied_by_the_runtime") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        // `{"value": "x" * 100}`: valid input, larger than the configured 64 bytes.
                        let mut payload = vec![0xA1u8, 0x65, b'v', b'a', b'l', b'u', b'e', 0x78, 100];
                        payload.extend(::std::iter::repeat(b'x').take(100));
                        let request = || {
                            #{Http}::Request::builder()
                                .method("POST")
                                .uri("/service/Example/operation/getFoo")
                                .header("content-type", "application/cbor")
                                .header("smithy-protocol", "rpc-v2-cbor")
                                .body(#{Server}::body::Body::from_bytes(payload.clone().into()))
                                .unwrap()
                        };
                        let builder = || {
                            crate::Example::builder(crate::ExampleConfig::builder().build())
                                .get_foo(|_input: crate::input::GetFooInput| async { crate::output::GetFooOutput {} })
                        };
                        let limited = builder().build().unwrap();
                        assert_eq!(limited.oneshot(request()).await.unwrap().status(), 400);
                        // Replacing the body configuration can explicitly remove the generated limit.
                        let unlimited = builder()
                            .request_body_config(#{Server}::config::ServiceRequestBodyConfig::default())
                            .build()
                            .unwrap();
                        assert_eq!(unlimited.oneshot(request()).await.unwrap().status(), 200);
                        // An operation override lifts the limit, so the payload itself is acceptable.
                        let lifted = builder()
                            .request_body_config(#{Server}::config::ServiceRequestBodyConfig::default()
                                .with_global(#{Server}::config::RequestBodyCollectionConfig::default()
                                    .with_max_bytes(::std::num::NonZeroUsize::new(64)))
                                .with_per_operation(
                                [("test##getFoo".to_owned(), #{Server}::config::RequestBodyCollectionConfig::default())].into(),
                            ))
                            .build()
                            .unwrap();
                        assert_eq!(lifted.oneshot(request()).await.unwrap().status(), 200);
                        """,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        *RuntimeType.preludeScope,
                    )
                }
            }
        }
    }

    @Test
    fun `restXml reads modeled collection children by default`() {
        restXmlCollectionElementNames(strictCollectionElementNames = null, tags = 1, attrs = 1)
        restXmlCollectionElementNames(strictCollectionElementNames = true, tags = 1, attrs = 1)
    }

    @Test
    fun `restXml collection names can be configured independently`() {
        restXmlCollectionElementNames(strictCollectionElementNames = false, tags = 2, attrs = 2)
    }

    /**
     * Sends a restXml request whose wrapped list and map each hold one child named as the model says
     * (`member`, `entry`) and one that is not, and checks how many of them the handler receives under
     * `customizationConfig.protocols."aws.protocols#restXml".strictCollectionElementNames`.
     */
    private fun restXmlCollectionElementNames(
        strictCollectionElementNames: Boolean?,
        tags: Int,
        attrs: Int,
    ) {
        val model =
            """
            namespace test
            use aws.protocols#restXml
            @restXml
            service Example { operations: [Collect] }
            @http(method: "POST", uri: "/collect")
            operation Collect {
                input := { tags: TagList attrs: AttrMap }
                output := {
                    @httpHeader("x-tags") tags: Integer
                    @httpHeader("x-attrs") attrs: Integer
                }
            }
            list TagList { member: String }
            map AttrMap { key: String value: String }
            """.asSmithyModel(smithyVersion = "2")
        val settings =
            ServerAdditionalSettings.builder().schemaSerde().toObjectNode().toBuilder()
        if (strictCollectionElementNames != null) {
            settings.withMember(
                "customizationConfig",
                ObjectNode.builder().withMember(
                    "protocols",
                    ObjectNode.builder()
                        .withMember(
                            "aws.protocols#restXml",
                            ObjectNode.builder().withMember("strictCollectionElementNames", strictCollectionElementNames).build(),
                        )
                        .build(),
                ).build(),
            )
        }
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = settings.build()),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("wrapped_collections_read_the_expected_children") {
                    rustTemplate(
                        """
                        use #{Tower}::ServiceExt;
                        let service = crate::Example::builder(crate::ExampleConfig::builder().build())
                            .collect(|input: crate::input::CollectInput| async move {
                                crate::output::CollectOutput {
                                    tags: input.tags.map(|tags| tags.len() as i32),
                                    attrs: input.attrs.map(|attrs| attrs.len() as i32),
                                }
                            })
                            .build()
                            .unwrap();
                        let body = "<CollectInput>\
                            <tags><item>a</item><member>b</member></tags>\
                            <attrs>\
                            <item><key>x</key><value>9</value></item>\
                            <entry><key>a</key><value>1</value></entry>\
                            </attrs>\
                            </CollectInput>";
                        let request = #{Http}::Request::builder()
                            .method("POST")
                            .uri("/collect")
                            .header("content-type", "application/xml")
                            .body(#{Server}::body::Body::from_bytes(body.as_bytes().to_vec().into()))
                            .unwrap();
                        let response = service.oneshot(request).await.unwrap();
                        assert_eq!(response.status(), 200);
                        assert_eq!(response.headers().get("x-tags").unwrap(), "$tags");
                        assert_eq!(response.headers().get("x-attrs").unwrap(), "$attrs");
                        """,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        *RuntimeType.preludeScope,
                    )
                }
            }
        }
    }

    private fun testProtocolRegistry(
        codegenContext: ServerCodegenContext,
        bodyRouting: Boolean,
        protocolIds: List<String> = listOf("test#bodyRouting"),
    ): RuntimeType =
        RuntimeType.forInlineFun("REGISTRY", RustModule.private("test_protocol")) {
            val shapeIds =
                protocolIds.joinToString(", ") { id ->
                    "#{Schema}::shape_id!(${id.substringBefore("#").dq()}, ${id.substringAfter("#").dq()})"
                }
            val registrationMethod =
                if (bodyRouting) {
                    "body_routed"
                } else {
                    "metadata_routed"
                }
            val registrations =
                protocolIds.mapIndexed { index, id ->
                    val order =
                        if (index == 0) {
                            ""
                        } else {
                            ".with_order(&[#{Server}::schema::ProtocolOrder::After(${protocolIds[index - 1].replace("#", "##").dq()})])"
                        }
                    "ProtocolRegistration::$registrationMethod::<TestProtocol<$index>>(${id.replace("#", "##").dq()})$order,"
                }.joinToString("\n")
            val routing =
                if (bodyRouting) {
                    """
                    impl<const INDEX: usize> #{Server}::schema::BodyRoutedProtocol for TestProtocol<INDEX> {
                        fn from_build_context(ctx: &ProtocolBuildContext<'_>) -> #{Result}<Self, RouterBuildError> {
                            assert!(ctx.service.protocols().contains(TestProtocol::<INDEX>.protocol_id()));
                            #{Ok}(TestProtocol)
                        }
                        fn build_router(&self, _ctx: RouterBuildContext<'_>)
                            -> #{Result}<impl BodyProtocolRouter + 'static + use<INDEX>, RouterBuildError> {
                            #{Ok}(BodyRouter)
                        }
                    }
                    """
                } else {
                    """
                    impl<const INDEX: usize> #{Server}::schema::MetadataRoutedProtocol for TestProtocol<INDEX> {
                        fn from_build_context(ctx: &ProtocolBuildContext<'_>) -> #{Result}<Self, RouterBuildError> {
                            assert!(ctx.service.protocols().contains(TestProtocol::<INDEX>.protocol_id()));
                            #{Ok}(TestProtocol)
                        }
                        fn build_router(&self, ctx: RouterBuildContext<'_>)
                            -> #{Result}<impl #{Server}::schema::routing::MetadataProtocolRouter + 'static + use<INDEX>, RouterBuildError> {
                            // `use<INDEX>` admits delegating through a temporary protocol.
                            #{Server}::schema::MetadataRoutedProtocol::build_router(
                                &#{Server}::schema::protocol::RestJson1::default(), ctx)
                        }
                    }
                    """
                }
            rustTemplate(
                """
                use #{Server}::schema::{ProtocolBuildContext, ProtocolRegistration, ProtocolRegistry, ServerProtocol};
                use #{Server}::schema::routing::{BodyProtocolRouter, BodyRouteClaim, RouterBuildContext, RouterBuildError};
                use #{Server}::body::BoxBody;
                use #{Schema}::{Schema, ShapeId};
                use #{Http}::{Request, Response};
                ##[derive(Debug)] ##[allow(dead_code)] // Unused on the metadata-routed pass.
                struct BodyRouter;
                impl BodyProtocolRouter for BodyRouter {
                    // These tests build the service; no request routes through this protocol.
                    fn claim(&self, _: &Request<()>) -> BodyRouteClaim { unreachable!() }
                }
                ##[derive(Debug)]
                struct TestProtocol<const INDEX: usize>;
                impl<const INDEX: usize> ServerProtocol for TestProtocol<INDEX> {
                    fn serialize_runtime_error(&self, _: #{Server}::runtime_error::RuntimeError) -> Response<BoxBody> { unreachable!() }
                    fn protocol_id(&self) -> &'static ShapeId<'static> {
                        static IDS: [ShapeId<'static>; ${protocolIds.size}] = [$shapeIds];
                        &IDS[INDEX]
                    }
                    fn deserialize_request<'a>(&'a self, _: &Schema<'_>, _: &'a #{RuntimeApi}::http::Request<#{Bytes}>)
                        -> #{Result}<#{Box}<dyn #{Schema}::serde::ShapeDeserializer + 'a>, #{Server}::schema::DeserializeError> { unreachable!() }
                    fn serialize_response(&self, _: &Schema<'_>, _: &dyn #{Schema}::serde::SerializableStruct) -> Response<BoxBody> { unreachable!() }
                    fn serialize_streaming_response(&self, _: &Schema<'_>, _: &dyn #{Schema}::serde::SerializableStruct, _: &Schema<'_>, _: Option<BoxBody>) -> Response<BoxBody> { unreachable!() }
                    fn serialize_error(&self, _: &dyn #{Server}::schema::HttpModeledError) -> Response<BoxBody> { unreachable!() }
                    fn serialize_rejection(&self, error: #{Server}::schema::DeserializeError) -> Response<BoxBody> {
                        let mut response = #{Server}::schema::protocol::RestJson1::default().serialize_rejection(error);
                        response.headers_mut().insert("x-test-protocol", INDEX.to_string().parse().unwrap());
                        response
                    }
                }
                $routing
                pub(crate) static REGISTRY: ProtocolRegistry = ProtocolRegistry::new(&[
                    $registrations
                ]);
                """,
                "Server" to ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType(),
                "Schema" to RuntimeType.smithySchema(codegenContext.runtimeConfig),
                "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                "Bytes" to RuntimeType.Bytes,
                "RuntimeApi" to RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig),
                *RuntimeType.preludeScope,
            )
        }
}
