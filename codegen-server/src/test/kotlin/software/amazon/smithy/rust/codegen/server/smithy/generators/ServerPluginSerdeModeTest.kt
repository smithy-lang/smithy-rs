/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class ServerPluginSerdeModeTest {
    @ParameterizedTest
    @ValueSource(ints = [0, 1, 2])
    fun `one HTTP plugin converts failures in generated schema and tuple modes`(mode: Int) {
        val model =
            """
            namespace test
            use aws.protocols#restJson1
            use smithy.protocols#rpcv2Cbor

            @restJson1
            ${if (mode == 2) "@rpcv2Cbor" else ""}
            service Example { operations: [Greet] }

            @http(method: "GET", uri: "/greet")
            operation Greet { input := {} output := {} }
            """.asSmithyModel(smithyVersion = "2")
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = "test#Example",
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(mode != 0).toObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            val scope =
                arrayOf(
                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    "Http" to RuntimeType.http(context.runtimeConfig),
                    "Tower" to ServerCargoDependency.tower(context).toType(),
                    "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                    "Bytes" to RuntimeType.Bytes,
                    *RuntimeType.preludeScope,
                )
            crate.testModule {
                rustTemplate(
                    """
                    use #{Server}::response::IntoResponseWithRequest;
                    use #{Server}::extension::OperationExtensionExt;
                    use #{Server}::instrumentation::InstrumentExt;
                    use #{Tower}::ServiceExt;
                    use #{BodyUtil}::BodyExt;

                    struct ExistingPlugin;
                    impl<Ser, Op, S> #{Server}::plugin::Plugin<Ser, Op, S> for ExistingPlugin {
                        type Output = S;
                        fn apply(&self, input: S) -> S { input }
                    }
                    impl #{Server}::plugin::HttpMarker for ExistingPlugin {}

                    struct RejectPlugin;
                    impl #{Server}::plugin::HttpMarker for RejectPlugin {}
                    impl<Ser, Op, S, SerdeMode> #{Server}::plugin::Plugin<Ser, Op, S, SerdeMode> for RejectPlugin
                    where
                        Ser: #{Server}::service::ServiceShape,
                        #{Server}::runtime_error::InternalFailureException:
                            IntoResponseWithRequest<Ser::Protocol, SerdeMode>,
                    {
                        type Output = Reject<Ser::Protocol, SerdeMode, S>;
                        fn apply(&self, inner: S) -> Self::Output {
                            Reject { inner, marker: ::std::marker::PhantomData }
                        }
                    }

                    struct Reject<P, SerdeMode, S> {
                        inner: S,
                        marker: ::std::marker::PhantomData<fn() -> (P, SerdeMode)>,
                    }
                    impl<P, SerdeMode, S: Clone> Clone for Reject<P, SerdeMode, S> {
                        fn clone(&self) -> Self {
                            Self { inner: self.inner.clone(), marker: ::std::marker::PhantomData }
                        }
                    }
                    impl<P, SerdeMode, S, B> #{Tower}::Service<#{Http}::Request<B>> for Reject<P, SerdeMode, S>
                    where
                        #{Server}::runtime_error::InternalFailureException: IntoResponseWithRequest<P, SerdeMode>,
                    {
                        type Response = #{Server}::response::Response;
                        type Error = ::std::convert::Infallible;
                        type Future = ::std::future::Ready<#{Result}<Self::Response, Self::Error>>;
                        fn poll_ready(&mut self, _: &mut ::std::task::Context<'_>) -> ::std::task::Poll<#{Result}<(), Self::Error>> {
                            ::std::task::Poll::Ready(#{Ok}(()))
                        }
                        fn call(&mut self, request: #{Http}::Request<B>) -> Self::Future {
                            ::std::future::ready(#{Ok}(
                                #{Server}::runtime_error::InternalFailureException.into_response_with_request(&request)
                            ))
                        }
                    }
                    """,
                    *scope,
                )
                tokioTest("middleware_uses_the_service_serde_mode") {
                    rustTemplate(
                        """
                        let plugins = #{Server}::plugin::HttpPlugins::new()
                            .push_legacy(ExistingPlugin)
                            .insert_operation_extension()
                            .instrument()
                            .push(RejectPlugin);
                        let config = crate::ExampleConfig::builder().http_plugin(plugins).build();
                        let service = crate::Example::builder${if (mode == 0) "::<#{Server}::body::Body, _, _, _>" else ""}(config)
                            .greet::<_, (), ()>(|_: crate::input::GreetInput| async {
                                panic!("rejecting middleware reached its handler");
                                ##[allow(unreachable_code)]
                                crate::output::GreetOutput::builder().build()
                            })
                            .build_unchecked();

                        let request = #{Http}::Request::builder().uri("/greet")
                            .body(#{Server}::body::Body::new(#{BodyUtil}::Full::new(#{Bytes}::new()))).unwrap();
                        let response = service.clone().oneshot(request).await.unwrap();
                        assert_eq!(response.status(), 500);
                        assert_eq!(response.headers()["content-type"], "application/json");
                        assert_eq!(response.headers()["x-amzn-errortype"], "InternalFailureException");
                        assert_eq!(response.headers().len(), 2);
                        assert_eq!(response.extensions().get::<#{Server}::extension::RuntimeErrorExtension>().unwrap().to_string(), "InternalFailureException");
                        assert_eq!(response.extensions().get::<#{Server}::extension::OperationExtension>().unwrap().0.absolute(), "test##Greet");
                        assert_eq!(response.extensions().len(), 2);
                        assert_eq!(response.into_body().collect().await.unwrap().to_bytes().as_ref(), b"{}");

                        if ${mode == 2} {
                            let request = #{Http}::Request::builder().method("POST")
                                .uri("/service/Example/operation/Greet")
                                .header("content-type", "application/cbor")
                                .header("smithy-protocol", "rpc-v2-cbor")
                                .body(#{Server}::body::Body::new(#{BodyUtil}::Full::new(#{Bytes}::from_static(b"\xa0")))).unwrap();
                            let response = service.oneshot(request).await.unwrap();
                            assert_eq!(response.status(), 500);
                            assert_eq!(response.headers()["content-type"], "application/cbor");
                            assert_eq!(response.headers().len(), 1);
                            assert_eq!(response.extensions().get::<#{Server}::extension::RuntimeErrorExtension>().unwrap().to_string(), "InternalFailureException");
                            assert_eq!(response.extensions().get::<#{Server}::extension::OperationExtension>().unwrap().0.absolute(), "test##Greet");
                            assert_eq!(response.extensions().len(), 2);
                            assert_eq!(response.into_body().collect().await.unwrap().to_bytes().as_ref(), b"\xa0");
                        }
                        """,
                        *scope,
                    )
                }
            }
        }
    }
}
