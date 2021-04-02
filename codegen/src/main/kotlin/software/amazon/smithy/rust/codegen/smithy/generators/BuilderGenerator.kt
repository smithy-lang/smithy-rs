/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.Default
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.defaultValue
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.makeOptional
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

fun StructureShape.builderSymbol(symbolProvider: RustSymbolProvider): RuntimeType {
    val symbol = symbolProvider.toSymbol(this)
    return RuntimeType("Builder", null, "${symbol.namespace}::${symbol.name.toSnakeCase()}")
}

class ModelBuilderGenerator(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val shape: StructureShape
) :
    BuilderGenerator(model, symbolProvider, shape) {
    override fun buildFn(implBlockWriter: RustWriter) {
        val fallibleBuilder = StructureGenerator.fallibleBuilder(shape, symbolProvider)
        val returnType = when (fallibleBuilder) {
            true -> "Result<#T, String>"
            false -> "#T"
        }
        val outputSymbol = symbolProvider.toSymbol(shape)
        implBlockWriter.docs("Consumes the builder and constructs a #D", outputSymbol)
        implBlockWriter.rustBlock("pub fn build(self) -> $returnType", outputSymbol) {
            conditionalBlock("Ok(", ")", conditional = fallibleBuilder) {
                // If a wrapper is specified, use the `::new` associated function to construct the wrapper
                coreBuilder(this)
            }
        }
    }

    override fun RustWriter.missingRequiredField(field: String) {
        val errMessage = "$field is required when building ${symbolProvider.toSymbol(shape).name}"
        rust(errMessage.dq())
    }
}

fun RuntimeConfig.operationBuildError() = RuntimeType.operationModule(this).member("BuildError")

class OperationInputBuilderGenerator(
    model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val shape: OperationShape,
    private val serviceName: String,
    private val features: List<OperationCustomization>,
) : BuilderGenerator(model, symbolProvider, shape.inputShape(model)) {
    private val runtimeConfig = symbolProvider.config().runtimeConfig

    override fun RustWriter.missingRequiredField(field: String) {
        val detailedMessage = "$field was not specified but it is required when building ${
        symbolProvider.toSymbol(
            shape
        ).name
        }"
        rust(
            """#T::MissingField { field: ${field.dq()}, details: ${detailedMessage.dq()}}""",
            runtimeConfig.operationBuildError()
        )
    }

    override fun buildFn(implBlockWriter: RustWriter) {
        val outputSymbol = symbolProvider.toSymbol(shape)
        val operationT = RuntimeType.operation(runtimeConfig)
        val operationModule = RuntimeType.operationModule(runtimeConfig)
        val sdkBody = RuntimeType.sdkBody(runtimeConfig)
        val retryType = features.mapNotNull { it.retryType() }.firstOrNull()?.let { implBlockWriter.format(it) } ?: "()"

        val baseReturnType = with(implBlockWriter) { "${format(operationT)}<${format(outputSymbol)}, $retryType>" }
        val returnType = "Result<$baseReturnType, ${implBlockWriter.format(runtimeConfig.operationBuildError())}>"

        implBlockWriter.docs("Consumes the builder and constructs an Operation<#D>", outputSymbol)
        // For codegen simplicity, allow `let x = ...; x`
        implBlockWriter.rust("##[allow(clippy::let_and_return)]")
        implBlockWriter.rustBlock("pub fn build(self, _config: &#T::Config) -> $returnType", RuntimeType.Config) {
            withBlock("Ok({", "})") {
                withBlock("let op = #T::new(", ");", outputSymbol) {
                    coreBuilder(this)
                }
                rust(
                    """
                    ##[allow(unused_mut)]
                    let mut request = #T::Request::new(op.build_http_request()?.map(#T::from));
                """,
                    operationModule, sdkBody
                )
                features.forEach { it.section(OperationSection.MutateRequest("request", "_config"))(this) }
                rust(
                    """
                    let op = #1T::Operation::new(
                        request,
                        op
                    ).with_metadata(#1T::Metadata::new(${shape.id.name.dq()}, ${serviceName.dq()}));
                """,
                    operationModule,
                )
                features.forEach { it.section(OperationSection.FinalizeOperation("op", "_config"))(this) }
                rust("op")
            }
        }
    }
}

/** setter names will never hit a reserved word and therefore never need escaping */
fun MemberShape.setterName(): String = "set_${this.memberName.toSnakeCase()}"

abstract class BuilderGenerator(
    val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val shape: StructureShape
) {
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val structureSymbol = symbolProvider.toSymbol(shape)
    fun render(writer: RustWriter) {
        val symbol = symbolProvider.toSymbol(shape)
        // TODO: figure out exactly what docs we want on a the builder module
        writer.docs("See #D", symbol)
        // check(writer.namespace == shape.builderSymbol(symbolProvider).namespace)
        val segments = shape.builderSymbol(symbolProvider).namespace.split("::")
        writer.withModule(segments.last()) {
            renderBuilder(this)
        }
    }

    fun renderConvenienceMethod(implBlock: RustWriter) {
        val builderSymbol = shape.builderSymbol(symbolProvider)
        implBlock.docs("Creates a new builder-style object to manufacture #D", structureSymbol)
        implBlock.rustBlock("pub fn builder() -> #T", builderSymbol) {
            write("#T::default()", builderSymbol)
        }
    }

    private fun renderBuilder(writer: RustWriter) {
        val builderName = "Builder"

        val symbol = structureSymbol
        writer.docs("A builder for #D", symbol)
        writer.write("##[non_exhaustive]")
        writer.write("##[derive(Debug, Clone, Default)]")
        writer.rustBlock("pub struct $builderName") {
            members.forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                // All fields in the builder are optional
                val memberSymbol = symbolProvider.toSymbol(member).makeOptional()
                // TODO: should the builder members be public?
                write("$memberName: #T,", memberSymbol)
            }
        }

        fun builderConverter(coreType: RustType) = when (coreType) {
            is RustType.String,
            is RustType.Box -> "inp.into()"
            else -> "inp"
        }

        writer.rustBlock("impl $builderName") {
            members.forEach { member ->
                // All fields in the builder are optional
                val memberSymbol = symbolProvider.toSymbol(member)
                val outerType = memberSymbol.rustType()
                val coreType = outerType.stripOuter<RustType.Option>()
                val memberName = symbolProvider.toMemberName(member)
                // Render a context-aware builder method for certain types, eg. a method for vectors that automatically
                // appends
                when (coreType) {
                    is RustType.Vec -> renderVecHelper(memberName, coreType)
                    is RustType.HashMap -> renderMapHelper(memberName, coreType)
                    else -> {
                        val signature = when (coreType) {
                            is RustType.String,
                            is RustType.Box -> "(mut self, inp: impl Into<${coreType.render(true)}>) -> Self"
                            else -> "(mut self, inp: ${coreType.render(true)}) -> Self"
                        }
                        writer.documentShape(member, model)
                        writer.rustBlock("pub fn $memberName$signature") {
                            write("self.$memberName = Some(${builderConverter(coreType)});")
                            write("self")
                        }
                    }
                }

                // Render a `set_foo` method. This is useful as a target for code generation, because the argument type
                // is exactly the same as the resulting member type.
                writer.rustBlock("pub fn ${member.setterName()}(mut self, inp: ${outerType.render(true)}) -> Self") {
                    val v = "inp".letIf(outerType !is RustType.Option) {
                        "Some($it)"
                    }
                    rust("self.$memberName = $v; self")
                }
            }
            buildFn(this)
        }
    }

    private fun RustWriter.renderVecHelper(memberName: String, coreType: RustType.Vec) {
        rustBlock("pub fn $memberName(mut self, inp: ${coreType.member.render(true)}) -> Self") {
            rust(
                """
                let mut v = self.$memberName.unwrap_or_default();
                v.push(inp);
                self.$memberName = Some(v);
                self
            """
            )
        }
    }

    private fun RustWriter.renderMapHelper(memberName: String, coreType: RustType.HashMap) {
        rustBlock("pub fn $memberName(mut self, k: ${coreType.key.render(true)}, v: ${coreType.member.render(true)}) -> Self") {
            rust(
                """
                let mut hash_map = self.$memberName.unwrap_or_default();
                hash_map.insert(k, v);
                self.$memberName = Some(hash_map);
                self
            """
            )
        }
    }

    abstract fun RustWriter.missingRequiredField(field: String)

    abstract fun buildFn(implBlockWriter: RustWriter)

    /**
     * The core builder of the inner type. If the structure requires a fallible builder, this may use `?` to return
     * errors
     * ```rust
     * SomeStruct {
     *    field: builder.field,
     *    field2: builder.field2,
     *    field3: builder.field3.unwrap_or_default()
     *    field4: builder.field4.ok_or("field4 is required when building SomeStruct")?
     * }
     */
    protected fun coreBuilder(writer: RustWriter) {
        writer.rustBlock("#T", structureSymbol) {
            members.forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                val memberSymbol = symbolProvider.toSymbol(member)
                val default = memberSymbol.defaultValue()
                withBlock("$memberName: self.$memberName", ",") {
                    // Write the modifier
                    when {
                        !memberSymbol.isOptional() && default == Default.RustDefault -> rust(".unwrap_or_default()")
                        !memberSymbol.isOptional() -> withBlock(
                            ".ok_or(",
                            ")?"
                        ) { missingRequiredField(memberName) }
                        memberSymbol.isOptional() && default is Default.Custom -> {
                            withBlock(".or_else(||Some(", "))") { default.render(this) }
                        }
                    }
                }
            }
        }
    }
}
