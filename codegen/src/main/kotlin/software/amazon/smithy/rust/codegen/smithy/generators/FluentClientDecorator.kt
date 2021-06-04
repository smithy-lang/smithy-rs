/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.contains
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class FluentClientDecorator : RustCodegenDecorator {
    override val name: String = "FluentClient"
    override val order: Byte = 0

    private fun applies(protocolConfig: ProtocolConfig): Boolean = protocolConfig.symbolProvider.config().codegenConfig.includeFluentClient

    override fun extras(protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
        if (!applies(protocolConfig)) {
            return;
        }

        val module = RustMetadata(additionalAttributes = listOf(Attribute.Cfg.feature("client")), public = true)
        rustCrate.withModule(RustModule("client", module)) { writer ->
            FluentClientGenerator(protocolConfig).render(writer)
        }
        val smithyClient = CargoDependency.SmithyClient(protocolConfig.runtimeConfig)
        rustCrate.addFeature(Feature("client", true, listOf(smithyClient.name)))
        rustCrate.addFeature(Feature("rustls", default = true, listOf("smithy-client/rustls")))
        rustCrate.addFeature(Feature("native-tls", default = false, listOf("smithy-client/native-tls")))
    }

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        if (!applies(protocolConfig)) {
            return baseCustomizations;
        }

        return baseCustomizations + object : LibRsCustomization() {
            override fun section(section: LibRsSection) = when (section) {
                is LibRsSection.Body -> writable {
                    Attribute.Cfg.feature("client").render(this)
                    rust("pub use client::Client;")
                }
                else -> emptySection
            }
        }
    }
}

class FluentClientGenerator(protocolConfig: ProtocolConfig) {
    private val serviceShape = protocolConfig.serviceShape
    private val operations =
        TopDownIndex.of(protocolConfig.model).getContainedOperations(serviceShape).sortedBy { it.id }
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model
    private val clientDep = CargoDependency.SmithyClient(protocolConfig.runtimeConfig).copy(optional = true)
    private val runtimeConfig = protocolConfig.runtimeConfig

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            ##[derive(std::fmt::Debug)]
            pub(crate) struct Handle<C, M, R> {
                client: #{client}::Client<C, M, R>,
                conf: crate::Config,
            }

            ##[derive(Clone, std::fmt::Debug)]
            pub struct Client<C, M, R = #{client}::retry::Standard> {
                handle: std::sync::Arc<Handle<C, M, R>>
            }

            impl<C, M, R> From<#{client}::Client<C, M, R>> for Client<C, M, R> {
                fn from(client: #{client}::Client<C, M, R>) -> Self {
                    Self::with_config(client, crate::Config::builder().build())
                }
            }

            impl<C, M, R> Client<C, M, R> {
                pub fn with_config(client: #{client}::Client<C, M, R>, conf: crate::Config) -> Self {
                    Self {
                        handle: std::sync::Arc::new(Handle {
                            client,
                            conf,
                        })
                    }
                }

                pub fn conf(&self) -> &crate::Config {
                    &self.handle.conf
                }
            }
        """,
            "client" to clientDep.asType()
        )
        writer.rustBlockTemplate(
            """
            impl<C, M, R> Client<C, M, R>
              where
                C: #{client}::bounds::SmithyConnector,
                M: #{client}::bounds::SmithyMiddleware<C>,
                R: #{client}::retry::NewRequestPolicy,
            """,
                "client" to clientDep.asType(),
        ) {
            operations.forEach { operation ->
                val name = symbolProvider.toSymbol(operation).name
                rust(
                    """
                    pub fn ${name.toSnakeCase()}(&self) -> fluent_builders::$name<C, M, R> {
                        fluent_builders::$name::new(self.handle.clone())
                    }"""
                )
            }
        }
        writer.withModule("fluent_builders") {
            operations.forEach { operation ->
                val name = symbolProvider.toSymbol(operation).name
                val input = operation.inputShape(model)
                val members: List<MemberShape> = input.allMembers.values.toList()

                rust(
                    """
                ##[derive(std::fmt::Debug)]
                pub struct $name<C, M, R> {
                    handle: std::sync::Arc<super::Handle<C, M, R>>,
                    inner: #T
                }""",
                    input.builderSymbol(symbolProvider)
                )

                rustBlockTemplate(
                    """
                    impl<C, M, R> $name<C, M, R>
                      where
                        C: #{client}::bounds::SmithyConnector,
                        M: #{client}::bounds::SmithyMiddleware<C>,
                        R: #{client}::retry::NewRequestPolicy,
                    """,
                        "client" to CargoDependency.SmithyClient(runtimeConfig).asType(),
                ) {
                    rustTemplate(
                        """
                    pub(crate) fn new(handle: std::sync::Arc<super::Handle<C, M, R>>) -> Self {
                        Self { handle, inner: Default::default() }
                    }

                    pub async fn send(self) -> Result<#{ok}, #{sdk_err}<#{operation_err}>> where
                        R::Policy: #{client}::bounds::SmithyRetryPolicy<#{input}OperationOutputAlias, #{ok}, #{operation_err}, #{input}OperationRetryAlias>,
                    {
                        let input = self.inner.build().map_err(|err|#{sdk_err}::ConstructionFailure(err.into()))?;
                        let op = input.make_operation(&self.handle.conf)
                            .map_err(|err|#{sdk_err}::ConstructionFailure(err.into()))?;
                        self.handle.client.call(op).await
                    }
                    """,
                        "input" to symbolProvider.toSymbol(operation.inputShape(model)),
                        "ok" to symbolProvider.toSymbol(operation.outputShape(model)),
                        "operation_err" to operation.errorSymbol(symbolProvider),
                        "sdk_err" to CargoDependency.SmithyHttp(runtimeConfig).asType().copy(name = "result::SdkError"),
                        "client" to CargoDependency.SmithyClient(runtimeConfig).asType(),
                    )
                    members.forEach { member ->
                        val memberName = symbolProvider.toMemberName(member)
                        // All fields in the builder are optional
                        val memberSymbol = symbolProvider.toSymbol(member)
                        val outerType = memberSymbol.rustType()
                        val coreType = outerType.stripOuter<RustType.Option>()
                        when (coreType) {
                            is RustType.Vec -> renderVecHelper(member, memberName, coreType)
                            is RustType.HashMap -> renderMapHelper(member, memberName, coreType)
                            else -> {
                                val signature = when (coreType) {
                                    is RustType.String,
                                    is RustType.Box -> "(mut self, inp: impl Into<${coreType.render(true)}>) -> Self"
                                    else -> "(mut self, inp: ${coreType.render(true)}) -> Self"
                                }
                                documentShape(member, model)
                                rustBlock("pub fn $memberName$signature") {
                                    write("self.inner = self.inner.$memberName(inp);")
                                    write("self")
                                }
                            }
                        }
                        // pure setter
                        rustBlock("pub fn ${member.setterName()}(mut self, inp: ${outerType.render(true)}) -> Self") {
                            rust(
                                """
                                self.inner = self.inner.${member.setterName()}(inp);
                                self
                                """
                            )
                        }
                    }
                }
            }
        }
    }

    private fun RustWriter.renderMapHelper(member: MemberShape, memberName: String, coreType: RustType.HashMap) {
        documentShape(member, model)
        val k = coreType.key
        val v = coreType.member

        rustBlock("pub fn $memberName(mut self, k: impl Into<${k.render()}>, v: impl Into<${v.render()}>) -> Self") {
            rust(
                """
                self.inner = self.inner.$memberName(k, v);
                self
            """
            )
        }
    }

    private fun RustWriter.renderVecHelper(member: MemberShape, memberName: String, coreType: RustType.Vec) {
        documentShape(member, model)
        rustBlock("pub fn $memberName(mut self, inp: impl Into<${coreType.member.render(true)}>) -> Self") {
            rust(
                """
                self.inner = self.inner.$memberName(inp);
                self
            """
            )
        }
    }
}
