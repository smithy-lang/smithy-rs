/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import kotlin.io.path.readText

internal class ServerServiceGeneratorTest {
    /**
     * See <https://github.com/smithy-lang/smithy-rs/issues/3177>.
     */
    @Test
    fun `one should be able to return a built service from a function`() {
        val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

        val testDirs =
            serverIntegrationTest(model) { _, rustCrate ->
                rustCrate.testModule {
                    // No actual tests: we just want to check that this compiles.
                    rust(
                        """
                    fn _build_service() -> crate::SimpleService {
                        let config = crate::SimpleServiceConfig::builder().build();
                        let service = crate::SimpleService::builder(config).build_unchecked();

                        service.boxed()
                    }
                    """,
                    )
                }
            }

        // test the generated metadata for all generated projects (both HTTP 0.x and HTTP 1.x)
        testDirs.forEach { generatedServer ->
            val cargoToml = generatedServer.path.resolve("Cargo.toml").readText()
            assert(cargoToml.contains("codegen-version =")) { cargoToml }
            assert(cargoToml.contains("protocol = \"aws.protocols#restJson1\"")) { cargoToml }
        }
    }

    /**
     * The legacy (non-schema) builder must not require handlers, HTTP plugins or layers to be `Sync`:
     * only the schema router shares its routes across threads.
     */
    @Test
    fun `legacy handlers and http plugins need not be Sync`() {
        val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

        serverIntegrationTest(model) { codegenContext, rustCrate ->
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
                    "Tower" to ServerCargoDependency.Tower.toType(),
                    *RuntimeType.preludeScope,
                )
            }
        }
    }
}
