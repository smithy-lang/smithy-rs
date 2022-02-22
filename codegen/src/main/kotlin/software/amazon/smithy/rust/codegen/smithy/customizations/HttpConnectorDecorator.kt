/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

class HttpConnectorDecorator : RustCodegenDecorator {
    override val name: String = "HttpConnector"
    override val order: Byte = 10

    override fun configCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + HttpConnectorCustomizations(codegenContext)
    }

    override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseHttpConnector(codegenContext.runtimeConfig)
    }
}

// Generate path to the retry module in aws_smithy_types
fun smithyClientHttpConnectorModule(runtimeConfig: RuntimeConfig) =
    RuntimeType("http_connector", runtimeConfig.runtimeCrate("client"), "aws_smithy_client")

class PubUseHttpConnector(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                rust("pub use #T::HttpConnector;", smithyClientHttpConnectorModule(runtimeConfig))
            }
            else -> emptySection
        }
    }
}

private fun RuntimeConfig.httpConnector(): RuntimeType = RuntimeType("HttpConnector", this.runtimeCrate("client"), "aws_smithy_client::http_connector")

private class HttpConnectorCustomizations(codegenContext: CodegenContext) : ConfigCustomization() {
    private val codegenScope = arrayOf("HttpConnector" to codegenContext.runtimeConfig.httpConnector())

    override fun section(section: ServiceConfig): Writable = when (section) {
        is ServiceConfig.BuilderStruct -> writable {
            rustTemplate(
                "http_connector: Option<#{HttpConnector}>,",
                *codegenScope
            )
        }
        is ServiceConfig.BuilderImpl -> writable {
            rustTemplate(
                """
                /// Sets configuration used when making HTTP requests, like timeout and retry behavior
                pub fn http_connector(mut self, http_connector: #{HttpConnector}) -> Self {
                    self.set_http_connector(Some(http_connector));
                    self
                }

                /// Sets configuration used when making HTTP requests, like timeout and retry behavior
                pub fn set_http_connector(&mut self, http_connector: Option<#{HttpConnector}>) -> &mut Self {
                    self.http_connector = http_connector;
                    self
                }
                """,
                *codegenScope
            )
        }
        is ServiceConfig.BuilderBuild -> writable {
            rust("http_connector: self.http_connector,")
        }
        is ServiceConfig.ConfigStruct -> writable {
            rustTemplate("pub(crate) http_connector: Option<#{HttpConnector}>,", *codegenScope)
        }
        is ServiceConfig.ConfigImpl -> emptySection
        else -> emptySection
    }
}
