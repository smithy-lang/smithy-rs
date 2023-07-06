/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
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
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.canUseDefault
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.defaultValue
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.redactIfNecessary
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

// TODO(https://github.com/awslabs/smithy-rs/issues/1401) This builder generator is only used by the client.
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

fun RuntimeConfig.operationBuildError() = RuntimeType.operationModule(this).resolve("error::BuildError")
fun RuntimeConfig.serializationError() = RuntimeType.operationModule(this).resolve("error::SerializationError")

class OperationBuildError(private val runtimeConfig: RuntimeConfig) {
    fun missingField(field: String, details: String) = writable {
        rust("#T::missing_field(${field.dq()}, ${details.dq()})", runtimeConfig.operationBuildError())
    }
    fun invalidField(field: String, details: String) = invalidField(field) { rust(details.dq()) }
    fun invalidField(field: String, details: Writable) = writable {
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
        fun hasFallibleBuilder(structureShape: StructureShape, symbolProvider: SymbolProvider): Boolean =
            // All operation inputs should have fallible builders in case a new required field is added in the future.
            structureShape.hasTrait<SyntheticInputTrait>() ||
                structureShape
                    .members()
                    .map { symbolProvider.toSymbol(it) }.any {
                        // If any members are not optional && we can't use a default, we need to
                        // generate a fallible builder.
                        !it.isOptional() && !it.canUseDefault()
                    }

        fun renderConvenienceMethod(implBlock: RustWriter, symbolProvider: RustSymbolProvider, shape: StructureShape) {
            implBlock.docs("Creates a new builder-style object to manufacture #D.", symbolProvider.toSymbol(shape))
            symbolProvider.symbolForBuilder(shape).also { builderSymbol ->
                implBlock.rustBlock("pub fn builder() -> #T", builderSymbol) {
                    write("#T::default()", builderSymbol)
                }
            }
        }
    }

    private val runtimeConfig = symbolProvider.config.runtimeConfig
    private val members: List<MemberShape> = shape.allMembers.values.toList()
    private val structureSymbol = symbolProvider.toSymbol(shape)
    private val metadata = structureSymbol.expectRustMetadata()

    // Filter out any derive that isn't Debug, PartialEq, or Clone. Then add a Default derive
    private val builderDerives = metadata.derives.filter {
        it == RuntimeType.Debug || it == RuntimeType.PartialEq || it == RuntimeType.Clone
    } + RuntimeType.Default
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
        val returnType = when (fallibleBuilder) {
            true -> "#{Result}<${implBlockWriter.format(outputSymbol)}, ${implBlockWriter.format(runtimeConfig.operationBuildError())}>"
            false -> implBlockWriter.format(outputSymbol)
        }
        implBlockWriter.docs("Consumes the builder and constructs a #D.", outputSymbol)
        implBlockWriter.rustBlockTemplate("pub fn build(self) -> $returnType", *preludeScope) {
            conditionalBlockTemplate("#{Ok}(", ")", conditional = fallibleBuilder, *preludeScope) {
                // If a wrapper is specified, use the `::new` associated function to construct the wrapper
                coreBuilder(this)
            }
        }
    }

    private fun RustWriter.missingRequiredField(field: String) {
        val detailedMessage = "$field was not specified but it is required when building ${symbolProvider.toSymbol(shape).name}"
        OperationBuildError(runtimeConfig).missingField(field, detailedMessage)(this)
    }

    // TODO(EventStream): [DX] Consider updating builders to take EventInputStream as Into<EventInputStream>
    private fun renderBuilderMember(writer: RustWriter, memberName: String, memberSymbol: Symbol) {
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
        // TODO(https://github.com/awslabs/smithy-rs/issues/1302): This `asOptional()` call is superfluous except in
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
        // TODO(https://github.com/awslabs/smithy-rs/issues/1302): This `asOptional()` call is superfluous except in
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
        metadata.additionalAttributes.render(writer)
        Attribute(derive(builderDerives)).render(writer)
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
                    val fieldValue = member.redactIfNecessary(model, "self.$memberName")

                    rust(
                        "formatter.field(${memberName.dq()}, &$fieldValue);",
                    )
                }
                writeCustomizations(customizations, BuilderSection.AdditionalDebugFields(shape, "formatter"))
                rust("formatter.finish()")
            }
        }
    }

    private fun RustWriter.renderVecHelper(member: MemberShape, memberName: String, coreType: RustType.Vec) {
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

    private fun RustWriter.renderMapHelper(member: MemberShape, memberName: String, coreType: RustType.HashMap) {
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
                val default = memberSymbol.defaultValue()
                withBlock("$memberName: self.$memberName", ",") {
                    // Write the modifier
                    when {
                        !memberSymbol.isOptional() && default == Default.RustDefault -> rust(".unwrap_or_default()")
                        !memberSymbol.isOptional() -> withBlock(
                            ".ok_or_else(||",
                            ")?",
                        ) { missingRequiredField(memberName) }
                    }
                }
            }
            writeCustomizations(customizations, BuilderSection.AdditionalFieldsInBuild(shape))
        }
    }
}
