/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asArgument
import software.amazon.smithy.rust.codegen.core.rustlang.asOptional
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.docsTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.map
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.canUseDefault
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.core.util.shouldRedact
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

// TODO(https://github.com/smithy-lang/smithy-rs/issues/1401) This builder generator is only used by the client.
//  Move this entire file, and its tests, to `codegen-client`.

/** BuilderGenerator customization sections */
sealed class BuilderSection(name: String) : Section(name) {
    abstract val shape: StructureShape

    /** Hook to add additional fields to the builder */
    data class AdditionalFields(override val shape: StructureShape) : BuilderSection("AdditionalFields")

    /** Hook to add additional methods to the builder */
    data class AdditionalMethods(override val shape: StructureShape) : BuilderSection("AdditionalMethods")

    /** Hook to add additional fields to the `build()` method */
    data class AdditionalFieldsInBuild(override val shape: StructureShape) : BuilderSection("AdditionalFieldsInBuild")

    /** Hook to add additional fields to the `Debug` impl */
    data class AdditionalDebugFields(override val shape: StructureShape, val formatterName: String) :
        BuilderSection("AdditionalDebugFields")
}

/** Customizations for BuilderGenerator */
abstract class BuilderCustomization : NamedCustomization<BuilderSection>()

fun RuntimeConfig.operationBuildError() = RuntimeType.smithyTypes(this).resolve("error::operation::BuildError")

fun RuntimeConfig.serializationError() = RuntimeType.smithyTypes(this).resolve("error::operation::SerializationError")

fun MemberShape.enforceRequired(
    field: Writable,
    codegenContext: CodegenContext,
    produceOption: Boolean = true,
): Writable {
    if (!this.isRequired) {
        return field
    }
    val shape = this
    val isOptional = codegenContext.symbolProvider.toSymbol(shape).isOptional()
    val field = field.letIf(!isOptional) { it.map { t -> rust("Some(#T)", t) } }
    val error =
        OperationBuildError(codegenContext.runtimeConfig).missingField(
            codegenContext.symbolProvider.toMemberName(shape), "A required field was not set",
        )
    val unwrapped =
        when (codegenContext.model.expectShape(this.target)) {
            is StringShape ->
                writable {
                    rust("#T.filter(|f|!AsRef::<str>::as_ref(f).trim().is_empty())", field)
                }

            else -> field
        }.map { base -> rustTemplate("#{base}.ok_or_else(||#{error})?", "base" to base, "error" to error) }
    return unwrapped.letIf(produceOption) { w -> w.map { rust("Some(#T)", it) } }
}

class OperationBuildError(private val runtimeConfig: RuntimeConfig) {
    fun missingField(
        field: String,
        details: String,
    ) = writable {
        rust("#T::missing_field(${field.dq()}, ${details.dq()})", runtimeConfig.operationBuildError())
    }

    fun invalidField(
        field: String,
        details: String,
    ) = invalidField(field) { rust(details.dq()) }

    fun invalidField(
        field: String,
        details: Writable,
    ) = writable {
        rustTemplate(
            "#{error}::invalid_field(${field.dq()}, #{details:W})",
            "error" to runtimeConfig.operationBuildError(),
            "details" to details,
        )
    }
}

// Setter names will never hit a reserved word and therefore never need escaping.
fun MemberShape.setterName() = "set_${this.memberName.toSnakeCase()}"

// Getter names will never hit a reserved word and therefore never need escaping.
fun MemberShape.getterName() = "get_${this.memberName.toSnakeCase()}"

class BuilderGenerator(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    private val shape: StructureShape,
    private val customizations: List<BuilderCustomization>,
) {
    companion object {
        /**
         * Returns whether a structure shape, whose builder has been generated with [BuilderGenerator], requires a
         * fallible builder to be constructed.
         */
        fun hasFallibleBuilder(
            structureShape: StructureShape,
            symbolProvider: SymbolProvider,
        ): Boolean =
            // All operation inputs should have fallible builders in case a new required field is added in the future.
            structureShape.hasTrait<SyntheticInputTrait>() ||
                structureShape
                    .members()
                    .map { symbolProvider.toSymbol(it) }.any {
                        // If any members are not optional && we can't use a default, we need to
                        // generate a fallible builder.
                        !it.isOptional() && !it.canUseDefault()
                    }

        fun renderConvenienceMethod(
            implBlock: RustWriter,
            symbolProvider: RustSymbolProvider,
            shape: StructureShape,
        ) {
            implBlock.docs("Creates a new builder-style object to manufacture #D.", symbolProvider.toSymbol(shape))
            symbolProvider.symbolForBuilder(shape).also { builderSymbol ->
                implBlock.rustBlock("pub fn builder() -> #T", builderSymbol) {
                    write("#T::default()", builderSymbol)
                }
            }
        }

        fun renderIntoBuilderMethod(
            implBlock: RustWriter,
            symbolProvider: RustSymbolProvider,
            shape: StructureShape,
        ) {
            val members: List<MemberShape> = shape.allMembers.values.toList()
            symbolProvider.symbolForBuilder(shape).also { builderSymbol ->
                Attribute.AllowUnused.render(implBlock)
                implBlock.rustBlock("pub(crate) fn into_builder(self) -> #T", builderSymbol) {
                    write("Self::builder()")
                    for (member in members) {
                        val memberName = member.memberName.toSnakeCase()
                        val setter =
                            if (symbolProvider.toSymbol(member).isOptional()) {
                                member.setterName()
                            } else {
                                memberName
                            }
                        write(".$setter(self.$memberName)")
                    }
                }
            }
        }
    }

    private val runtimeConfig = symbolProvider.config.runtimeConfig
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val structureSymbol = symbolProvider.toSymbol(shape)
    private val metadata = structureSymbol.expectRustMetadata()

    // Filter out any derive that isn't Debug, PartialEq, or Clone. Then add a Default derive
    private val builderDerives =
        metadata.derives.filter {
            it == RuntimeType.Debug || it == RuntimeType.PartialEq || it == RuntimeType.Clone
        } + RuntimeType.Default

    // Filter out attributes
    private val builderAttributes =
        metadata.additionalAttributes.filter {
            it == Attribute.NonExhaustive
        }
    private val builderName = symbolProvider.symbolForBuilder(shape).name

    fun render(writer: RustWriter) {
        // Matching derives to the main structure + `Default` since we are a builder and everything is optional.
        renderBuilder(writer)
        if (!structureSymbol.expectRustMetadata().hasDebugDerive()) {
            renderDebugImpl(writer)
        }
    }

    private fun renderBuildFn(implBlockWriter: RustWriter) {
        val fallibleBuilder = hasFallibleBuilder(shape, symbolProvider)
        val outputSymbol = symbolProvider.toSymbol(shape)
        val returnType =
            when (fallibleBuilder) {
                true -> "#{Result}<${implBlockWriter.format(outputSymbol)}, ${implBlockWriter.format(runtimeConfig.operationBuildError())}>"
                false -> implBlockWriter.format(outputSymbol)
            }
        implBlockWriter.docs("Consumes the builder and constructs a #D.", outputSymbol)
        val trulyRequiredMembers = members.filter { trulyRequired(it) }
        if (trulyRequiredMembers.isNotEmpty()) {
            implBlockWriter.docs("This method will fail if any of the following fields are not set:")
            trulyRequiredMembers.forEach {
                val memberName = symbolProvider.toMemberName(it)
                implBlockWriter.docsTemplate(
                    // We have to remove the `r##` prefix in the path b/c Rustdoc doesn't support it.
                    "- [`$memberName`](#{struct}::${memberName.removePrefix("r##")})",
                    "struct" to symbolProvider.symbolForBuilder(shape),
                )
            }
        }
        implBlockWriter.rustBlockTemplate("pub fn build(self) -> $returnType", *preludeScope) {
            conditionalBlockTemplate("#{Ok}(", ")", conditional = fallibleBuilder, *preludeScope) {
                // If a wrapper is specified, use the `::new` associated function to construct the wrapper
                coreBuilder(this)
            }
        }
    }

    private fun RustWriter.missingRequiredField(field: String) {
        val detailedMessage =
            "$field was not specified but it is required when building ${symbolProvider.toSymbol(shape).name}"
        OperationBuildError(runtimeConfig).missingField(field, detailedMessage)(this)
    }

    // TODO(EventStream): [DX] Consider updating builders to take EventInputStream as Into<EventInputStream>
    private fun renderBuilderMember(
        writer: RustWriter,
        memberName: String,
        memberSymbol: Symbol,
    ) {
        // Builder members are crate-public to enable using them directly in serializers/deserializers.
        // During XML deserialization, `builder.<field>.take` is used to append to lists and maps.
        writer.write("pub(crate) $memberName: #T,", memberSymbol)
    }

    private fun renderBuilderMemberFn(
        writer: RustWriter,
        coreType: RustType,
        member: MemberShape,
        memberName: String,
    ) {
        val input = coreType.asArgument("input")

        writer.documentShape(member, model)
        if (member.isRequired) {
            writer.docs("This field is required.")
        }
        writer.deprecatedShape(member)
        writer.rustBlock("pub fn $memberName(mut self, ${input.argument}) -> Self") {
            rustTemplate("self.$memberName = #{Some}(${input.value});", *preludeScope)
            write("self")
        }
    }

    /**
     * Render a `set_foo` method. This is useful as a target for code generation, because the argument type
     * is the same as the resulting member type, and is always optional.
     */
    private fun renderBuilderMemberSetterFn(
        writer: RustWriter,
        outerType: RustType,
        member: MemberShape,
        memberName: String,
    ) {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/1302): This `asOptional()` call is superfluous except in
        //  the case where the shape is a `@streaming` blob, because [StreamingTraitSymbolProvider] always generates
        //  a non `Option`al target type: in all other cases the client generates `Option`al types.
        val inputType = outerType.asOptional()

        writer.documentShape(member, model)
        writer.deprecatedShape(member)
        writer.rustBlock("pub fn ${member.setterName()}(mut self, input: ${inputType.render(true)}) -> Self") {
            rust("self.$memberName = input; self")
        }
    }

    /**
     * Render a `get_foo` method. This is useful as a target for code generation, because the argument type
     * is the same as the resulting member type, and is always optional.
     */
    private fun renderBuilderMemberGetterFn(
        writer: RustWriter,
        outerType: RustType,
        member: MemberShape,
        memberName: String,
    ) {
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/1302): This `asOptional()` call is superfluous except in
        //  the case where the shape is a `@streaming` blob, because [StreamingTraitSymbolProvider] always generates
        //  a non `Option`al target type: in all other cases the client generates `Option`al types.
        val inputType = outerType.asOptional()

        writer.documentShape(member, model)
        writer.deprecatedShape(member)
        writer.rustBlock("pub fn ${member.getterName()}(&self) -> &${inputType.render(true)}") {
            rust("&self.$memberName")
        }
    }

    private fun renderBuilder(writer: RustWriter) {
        writer.docs("A builder for #D.", structureSymbol)
        Attribute(derive(builderDerives)).render(writer)
        this.builderAttributes.render(writer)
        writer.rustBlock("pub struct $builderName") {
            for (member in members) {
                val memberName = symbolProvider.toMemberName(member)
                // All fields in the builder are optional.
                val memberSymbol = symbolProvider.toSymbol(member).makeOptional()
                renderBuilderMember(this, memberName, memberSymbol)
            }
            writeCustomizations(customizations, BuilderSection.AdditionalFields(shape))
        }

        writer.rustBlock("impl $builderName") {
            for (member in members) {
                // All fields in the builder are optional.
                val memberSymbol = symbolProvider.toSymbol(member)
                val outerType = memberSymbol.rustType()
                val coreType = outerType.stripOuter<RustType.Option>()
                val memberName = symbolProvider.toMemberName(member)
                // Render a context-aware builder method for certain types, e.g. a method for vectors that automatically
                // appends.
                when (coreType) {
                    is RustType.Vec -> renderVecHelper(member, memberName, coreType)
                    is RustType.HashMap -> renderMapHelper(member, memberName, coreType)
                    else -> renderBuilderMemberFn(this, coreType, member, memberName)
                }

                renderBuilderMemberSetterFn(this, outerType, member, memberName)
                renderBuilderMemberGetterFn(this, outerType, member, memberName)
            }
            writeCustomizations(customizations, BuilderSection.AdditionalMethods(shape))
            renderBuildFn(this)
        }
    }

    private fun renderDebugImpl(writer: RustWriter) {
        writer.rustBlock("impl #T for $builderName", RuntimeType.Debug) {
            writer.rustBlock("fn fmt(&self, f: &mut #1T::Formatter<'_>) -> #1T::Result", RuntimeType.stdFmt) {
                rust("""let mut formatter = f.debug_struct(${builderName.dq()});""")
                members.forEach { member ->
                    val memberName = symbolProvider.toMemberName(member)
                    // If the struct is marked sensitive all fields get redacted, otherwise each field is determined on its own
                    val fieldValue =
                        if (shape.shouldRedact(model)) {
                            REDACTION
                        } else {
                            member.redactIfNecessary(
                                model,
                                "self.$memberName",
                            )
                        }

                    rust(
                        "formatter.field(${memberName.dq()}, &$fieldValue);",
                    )
                }
                writeCustomizations(customizations, BuilderSection.AdditionalDebugFields(shape, "formatter"))
                rust("formatter.finish()")
            }
        }
    }

    private fun RustWriter.renderVecHelper(
        member: MemberShape,
        memberName: String,
        coreType: RustType.Vec,
    ) {
        docs("Appends an item to `$memberName`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        documentShape(member, model, autoSuppressMissingDocs = false)
        deprecatedShape(member)
        val input = coreType.member.asArgument("input")

        rustBlock("pub fn $memberName(mut self, ${input.argument}) -> Self") {
            rustTemplate(
                """
                let mut v = self.$memberName.unwrap_or_default();
                v.push(${input.value});
                self.$memberName = #{Some}(v);
                self
                """,
                *preludeScope,
            )
        }
    }

    private fun RustWriter.renderMapHelper(
        member: MemberShape,
        memberName: String,
        coreType: RustType.HashMap,
    ) {
        docs("Adds a key-value pair to `$memberName`.")
        rust("///")
        docs("To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).")
        rust("///")
        documentShape(member, model, autoSuppressMissingDocs = false)
        deprecatedShape(member)
        val k = coreType.key.asArgument("k")
        val v = coreType.member.asArgument("v")

        rustBlock(
            "pub fn $memberName(mut self, ${k.argument}, ${v.argument}) -> Self",
        ) {
            rustTemplate(
                """
                let mut hash_map = self.$memberName.unwrap_or_default();
                hash_map.insert(${k.value}, ${v.value});
                self.$memberName = #{Some}(hash_map);
                self
                """,
                *preludeScope,
            )
        }
    }

    private fun trulyRequired(member: MemberShape) =
        symbolProvider.toSymbol(member).let {
            !it.isOptional() && !it.canUseDefault()
        }

    /**
     * The core builder of the inner type. If the structure requires a fallible builder, this may use `?` to return
     * errors.
     * ```rust
     * SomeStruct {
     *    field1: builder.field1,
     *    field2: builder.field2.unwrap_or_default()
     *    field3: builder.field3.ok_or("field3 is required when building SomeStruct")?
     * }
     * ```
     */
    private fun coreBuilder(writer: RustWriter) {
        writer.rustBlock("#T", structureSymbol) {
            members.forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                val memberSymbol = symbolProvider.toSymbol(member)
                withBlock("$memberName: self.$memberName", ",") {
                    val generator = DefaultValueGenerator(runtimeConfig, symbolProvider, model)
                    val default = generator.defaultValue(member)
                    if (!memberSymbol.isOptional()) {
                        if (default != null) {
                            if (default.isRustDefault) {
                                rust(".unwrap_or_default()")
                            } else if (default.complexType) {
                                rust(".unwrap_or_else(|| #T)", default.expr)
                            } else {
                                rust(".unwrap_or(#T)", default.expr)
                            }
                        } else {
                            withBlock(
                                ".ok_or_else(||",
                                ")?",
                            ) { missingRequiredField(memberName) }
                        }
                    }
                }
            }
            writeCustomizations(customizations, BuilderSection.AdditionalFieldsInBuild(shape))
        }
    }
}
