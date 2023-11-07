/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

internal class ServiceConfigGeneratorTest {
    @Test
    fun `it should inject an aws_auth method that configures an HTTP plugin and a model plugin`() {
        val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

        val decorator = object : ServerCodegenDecorator {
            override val name: String
                get() = "AWSAuth pre-applied middleware decorator"
            override val order: Byte
                get() = -69

            override fun configMethods(codegenContext: ServerCodegenContext): List<ConfigMethod> {
                val smithyHttpServer = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
                val codegenScope = arrayOf(
                    "SmithyHttpServer" to smithyHttpServer,
                )
                return listOf(
                    ConfigMethod(
                        name = "aws_auth",
                        docs = "Docs",
                        params = listOf(
                            Binding("auth_spec", RuntimeType.String),
                            Binding("authorizer", RuntimeType.U64),
                        ),
                        errorType = RuntimeType.std.resolve("io::Error"),
                        initializer = Initializer(
                            code = writable {
                                rustTemplate(
                                    """
                                    if authorizer != 69 {
                                        return Err(std::io::Error::new(std::io::ErrorKind::Other, "failure 1"));
                                    }

                                    if auth_spec.len() != 69 {
                                        return Err(std::io::Error::new(std::io::ErrorKind::Other, "failure 2"));
                                    }
                                    let authn_plugin = #{SmithyHttpServer}::plugin::IdentityPlugin;
                                    let authz_plugin = #{SmithyHttpServer}::plugin::IdentityPlugin;
                                    """,
                                    *codegenScope,
                                )
                            },
                            layerBindings = emptyList(),
                            httpPluginBindings = listOf(
                                Binding(
                                    "authn_plugin",
                                    smithyHttpServer.resolve("plugin::IdentityPlugin"),
                                ),
                            ),
                            modelPluginBindings = listOf(
                                Binding(
                                    "authz_plugin",
                                    smithyHttpServer.resolve("plugin::IdentityPlugin"),
                                ),
                            ),
                        ),
                        isRequired = true,
                    ),
                )
            }
        }

        serverIntegrationTest(model, additionalDecorators = listOf(decorator)) { _, rustCrate ->
            rustCrate.testModule {
                rust(
                    """
                    use crate::{SimpleServiceConfig, SimpleServiceConfigError};
                    use aws_smithy_http_server::plugin::IdentityPlugin;
                    use crate::server::plugin::PluginStack;
                    """,
                )

                unitTest("successful_config_initialization") {
                    rust(
                        """
                        let _: SimpleServiceConfig<
                            tower::layer::util::Identity,
                            // One HTTP plugin has been applied.
                            PluginStack<IdentityPlugin, IdentityPlugin>,
                            // One model plugin has been applied.
                            PluginStack<IdentityPlugin, IdentityPlugin>,
                        > = SimpleServiceConfig::builder()
                            .aws_auth("a".repeat(69).to_owned(), 69)
                            .expect("failed to configure aws_auth")
                            .build()
                            .unwrap();
                        """,
                    )
                }

                unitTest("wrong_aws_auth_auth_spec") {
                    rust(
                        """
                        let actual_err = SimpleServiceConfig::builder()
                            .aws_auth("a".to_owned(), 69)
                            .unwrap_err();
                        let expected = std::io::Error::new(std::io::ErrorKind::Other, "failure 2").to_string();
                        assert_eq!(actual_err.to_string(), expected);
                        """,
                    )
                }

                unitTest("wrong_aws_auth_authorizer") {
                    rust(
                        """
                        let actual_err = SimpleServiceConfig::builder()
                            .aws_auth("a".repeat(69).to_owned(), 6969)
                            .unwrap_err();
                        let expected = std::io::Error::new(std::io::ErrorKind::Other, "failure 1").to_string();
                        assert_eq!(actual_err.to_string(), expected);
                        """,
                    )
                }

                unitTest("aws_auth_not_configured") {
                    rust(
                        """
                        let actual_err = SimpleServiceConfig::builder().build().unwrap_err();
                        let expected = SimpleServiceConfigError::AwsAuthNotConfigured.to_string();
                        assert_eq!(actual_err.to_string(), expected);
                        """,
                    )
                }
            }
        }
    }

    @Test
    fun `it should inject an method that applies three non-required layers`() {
        val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

        val decorator = object : ServerCodegenDecorator {
            override val name: String
                get() = "ApplyThreeNonRequiredLayers"
            override val order: Byte
                get() = 69

            override fun configMethods(codegenContext: ServerCodegenContext): List<ConfigMethod> {
                val identityLayer = RuntimeType.Tower.resolve("layer::util::Identity")
                val codegenScope = arrayOf(
                    "Identity" to identityLayer,
                )
                return listOf(
                    ConfigMethod(
                        name = "three_non_required_layers",
                        docs = "Docs",
                        params = emptyList(),
                        errorType = null,
                        initializer = Initializer(
                            code = writable {
                                rustTemplate(
                                    """
                                    let layer1 = #{Identity}::new();
                                    let layer2 = #{Identity}::new();
                                    let layer3 = #{Identity}::new();
                                    """,
                                    *codegenScope,
                                )
                            },
                            layerBindings = listOf(
                                Binding("layer1", identityLayer),
                                Binding("layer2", identityLayer),
                                Binding("layer3", identityLayer),
                            ),
                            httpPluginBindings = emptyList(),
                            modelPluginBindings = emptyList(),
                        ),
                        isRequired = false,
                    ),
                )
            }
        }

        serverIntegrationTest(model, additionalDecorators = listOf(decorator)) { _, rustCrate ->
            rustCrate.testModule {
                unitTest("successful_config_initialization_applying_the_three_layers") {
                    rust(
                        """
                        let _: crate::SimpleServiceConfig<
                            // Three Tower layers have been applied.
                            tower::layer::util::Stack<
                                tower::layer::util::Identity,
                                tower::layer::util::Stack<
                                    tower::layer::util::Identity,
                                    tower::layer::util::Stack<
                                        tower::layer::util::Identity,
                                        tower::layer::util::Identity,
                                    >,
                                >,
                            >,
                            aws_smithy_http_server::plugin::IdentityPlugin,
                            aws_smithy_http_server::plugin::IdentityPlugin,
                        > = crate::SimpleServiceConfig::builder()
                            .three_non_required_layers()
                            .build();
                        """,
                    )
                }

                unitTest("successful_config_initialization_without_applying_the_three_layers") {
                    rust(
                        """
                        crate::SimpleServiceConfig::builder().build();
                        """,
                    )
                }
            }
        }
    }
}
