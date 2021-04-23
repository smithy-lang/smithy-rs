/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

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
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

class FluentClientDecorator : RustCodegenDecorator {
    override val name: String = "FluentClient"
    override val order: Byte = 0

    override fun extras(protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
        val module = RustMetadata(additionalAttributes = listOf(Attribute.Cfg.feature("client")), public = true)
        rustCrate.withModule(RustModule("client", module)) { writer ->
            FluentClientGenerator(protocolConfig).render(writer)
        }
        val awsHyper = protocolConfig.runtimeConfig.awsHyper().name
        rustCrate.addFeature(Feature("client", true, listOf(awsHyper)))
        rustCrate.addFeature(Feature("rustls", default = true, listOf("$awsHyper/rustls")))
        rustCrate.addFeature(Feature("native-tls", default = false, listOf("$awsHyper/native-tls")))
    }

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
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
    private val hyperDep = protocolConfig.runtimeConfig.awsHyper().copy(optional = true)
    private val runtimeConfig = protocolConfig.runtimeConfig

    fun render(writer: RustWriter) {
        writer.rustTemplate(
            """
            ##[derive(std::fmt::Debug)]
            pub(crate) struct Handle {
                client: #{aws_hyper}::Client<#{aws_hyper}::conn::Standard>,
                conf: crate::Config
            }

            ##[derive(Clone, std::fmt::Debug)]
            pub struct Client {
                handle: std::sync::Arc<Handle>
            }
        """,
            "aws_hyper" to hyperDep.asType()
        )
        writer.rustBlock("impl Client") {
            rustTemplate(
                """
                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn from_env() -> Self {
                    Self::from_conf_conn(crate::Config::builder().build(), #{aws_hyper}::conn::Standard::https())
                }

                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn from_conf(conf: crate::Config) -> Self {
                    Self::from_conf_conn(conf, #{aws_hyper}::conn::Standard::https())
                }

                pub fn from_conf_conn(conf: crate::Config, conn: #{aws_hyper}::conn::Standard) -> Self {
                    let client = #{aws_hyper}::Client::new(conn);
                    Self { handle: std::sync::Arc::new(Handle { conf, client })}
                }

                pub fn conf(&self) -> &crate::Config {
                    &self.handle.conf
                }

            """,
                "aws_hyper" to hyperDep.asType()
            )
            operations.forEach { operation ->
                val name = symbolProvider.toSymbol(operation).name
                rust(
                    """
                    pub fn ${name.toSnakeCase()}(&self) -> fluent_builders::$name {
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
                pub struct $name {
                    handle: std::sync::Arc<super::Handle>,
                    inner: #T
                }""",
                    input.builderSymbol(symbolProvider)
                )

                rustBlock("impl $name") {
                    rustTemplate(
                        """
                    pub(crate) fn new(handle: std::sync::Arc<super::Handle>) -> Self {
                        Self { handle, inner: Default::default() }
                    }

                    pub async fn send(self) -> Result<#{ok}, #{sdk_err}<#{operation_err}>> {
                        let op = self.inner.build(&self.handle.conf).map_err(|err|#{sdk_err}::ConstructionFailure(err.into()))?;
                        self.handle.client.call(op).await
                    }
                    """,
                        "ok" to symbolProvider.toSymbol(operation.outputShape(model)),
                        "operation_err" to operation.errorSymbol(symbolProvider),
                        "sdk_err" to CargoDependency.SmithyHttp(runtimeConfig).asType().copy(name = "result::SdkError")
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
