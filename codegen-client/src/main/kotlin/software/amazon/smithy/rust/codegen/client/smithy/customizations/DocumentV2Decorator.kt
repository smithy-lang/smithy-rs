/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asArgument
import software.amazon.smithy.rust.codegen.core.rustlang.asDeref
import software.amazon.smithy.rust.codegen.core.rustlang.asRef
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docsTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.isDeref
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.DocumentV2Companion
import software.amazon.smithy.rust.codegen.core.smithy.generators.DocumentV2Index
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName

/**
 * Gives every struct that holds `Document` values a private `DocumentV2` companion, found by
 * [DocumentV2Index], with public `_v2` accessors next to the member's unchanged `Document` API.
 *
 * - The struct stores the companion in a private `DocumentV2Slot` field and gets a `payload_v2()` getter.
 * - The builder stores it in its own field and gets `payload_v2`, `set_payload_v2`, and `get_payload_v2`.
 *   The `Document` and `DocumentV2` builder fields are independent, so clearing the companion lets the `Document`
 *   value take effect again.
 * - `build()` keeps the companion and, when it is set, derives the public `Document` field from it with
 *   `DocumentV2::to_document_lossy`. Required-member checks and defaults apply to that derived value, so setting
 *   either form satisfies a required member.
 * - `into_builder()` carries the companion.
 * - Equality and `Debug` output are unchanged: the slot always compares equal, and these structures use the manual
 *   `Debug` impls, which print only the modeled members.
 *
 * Fluent builders get the same methods from [renderDocumentV2FluentMethods], which `FluentBuilderGenerator` calls.
 */
class DocumentV2Decorator : ClientCodegenDecorator {
    override val name: String = "DocumentV2"
    override val order: Byte = 0

    override fun symbolProvider(base: RustSymbolProvider): RustSymbolProvider = DocumentV2MetadataProvider(base)

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> = baseCustomizations + DocumentV2StructureCustomization(codegenContext)

    override fun builderCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<BuilderCustomization>,
    ): List<BuilderCustomization> = baseCustomizations + DocumentV2BuilderCustomization(codegenContext)
}

/**
 * Removes the `Debug` derive from structures with companions, so that the structure and its builder use the manual
 * `Debug` impls. Those print the modeled members, and leave out the companions.
 */
private class DocumentV2MetadataProvider(private val base: RustSymbolProvider) : SymbolMetadataProvider(base) {
    private val index = DocumentV2Index.of(model)

    override fun structureMeta(structureShape: StructureShape): RustMetadata {
        val baseMetadata = base.toSymbol(structureShape).expectRustMetadata()
        return if (index.hasCompanions(structureShape)) {
            baseMetadata.withoutDerives(RuntimeType.Debug)
        } else {
            baseMetadata
        }
    }

    override fun memberMeta(memberShape: MemberShape) = base.toSymbol(memberShape).expectRustMetadata()

    override fun unionMeta(unionShape: UnionShape) = base.toSymbol(unionShape).expectRustMetadata()

    override fun enumMeta(stringShape: StringShape) = base.toSymbol(stringShape).expectRustMetadata()

    override fun listMeta(listShape: ListShape) = base.toSymbol(listShape).expectRustMetadata()

    override fun mapMeta(mapShape: MapShape) = base.toSymbol(mapShape).expectRustMetadata()

    override fun stringMeta(stringShape: StringShape) = base.toSymbol(stringShape).expectRustMetadata()

    override fun numberMeta(numberShape: NumberShape) = base.toSymbol(numberShape).expectRustMetadata()

    override fun blobMeta(blobShape: BlobShape) = base.toSymbol(blobShape).expectRustMetadata()
}

private fun documentV2Scope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> {
    val slotModule = RuntimeType.forInlineDependency(InlineDependency.documentV2Slot(runtimeConfig))
    return arrayOf(
        *preludeScope,
        "DocumentV2" to RuntimeType.documentV2(runtimeConfig),
        "DocumentV2Slot" to slotModule.resolve("DocumentV2Slot"),
        "public_value" to slotModule.resolve("public_value"),
    )
}

/**
 * Caches [DocumentV2Index.companions] by shape, because generators ask for a section once per member, as with
 * [BuilderSection.MemberValueInBuild].
 */
private class CompanionCache(private val codegenContext: ClientCodegenContext) {
    private val index = DocumentV2Index.of(codegenContext.model)
    private val companions = mutableMapOf<ShapeId, List<DocumentV2Companion>>()

    fun of(shape: StructureShape): List<DocumentV2Companion> =
        companions.getOrPut(shape.id) { index.companions(shape, codegenContext.symbolProvider) }
}

private class DocumentV2StructureCustomization(private val codegenContext: ClientCodegenContext) :
    StructureCustomization() {
    private val scope = documentV2Scope(codegenContext.runtimeConfig)
    private val companionCache = CompanionCache(codegenContext)

    override fun section(section: StructureSection): Writable =
        writable {
            val companions = companionCache.of(section.shape)
            if (companions.isEmpty()) {
                return@writable
            }
            when (section) {
                is StructureSection.AdditionalFields ->
                    companions.forEach { companion ->
                        rustTemplate(
                            "${companion.name}: #{DocumentV2Slot}<#{Type}>,",
                            *scope,
                            "Type" to companion.type,
                        )
                    }

                is StructureSection.AdditionalTraitImpls ->
                    rustBlock("impl ${section.structName}") {
                        companions.forEach { renderGetter(it) }
                    }

                else -> {}
            }
        }

    private fun RustWriter.renderGetter(companion: DocumentV2Companion) {
        val optional = RustType.Option(companion.type)
        val (returnType, conversion) =
            if (companion.type.isDeref()) {
                optional.asDeref() to "as_deref"
            } else {
                optional.asRef() to "as_ref"
            }
        val field = companion.unescapedMemberName
        rustTemplate(
            """
            /// Returns the [`DocumentV2`](#{DocumentV2}) form of `$field`, if the builder set one.
            ///
            /// When this returns `Some`, the `$field` field holds the same value with each `DocumentV2`
            /// converted by [`DocumentV2::to_document_lossy`](#{DocumentV2}::to_document_lossy).
            """,
            *scope,
        )
        deprecatedShape(companion.member)
        rustTemplate(
            """
            pub fn ${companion.name}(&self) -> #{ReturnType} {
                self.${companion.name}.0.$conversion()
            }
            """,
            "ReturnType" to returnType,
        )
    }
}

private class DocumentV2BuilderCustomization(private val codegenContext: ClientCodegenContext) :
    BuilderCustomization() {
    private val scope = documentV2Scope(codegenContext.runtimeConfig)
    private val companionCache = CompanionCache(codegenContext)

    override fun section(section: BuilderSection): Writable =
        writable {
            val companions = companionCache.of(section.shape)
            if (companions.isEmpty()) {
                return@writable
            }
            when (section) {
                is BuilderSection.AdditionalFields ->
                    companions.forEach { companion ->
                        rustTemplate(
                            "pub(crate) ${companion.name}: #{Option}<#{Type}>,",
                            *scope,
                            "Type" to companion.type,
                        )
                    }

                is BuilderSection.AdditionalMethods ->
                    companions.forEach { companion ->
                        val field = "self.${companion.name}"
                        renderDocumentV2Methods(
                            companion,
                            codegenContext.runtimeConfig,
                            valueBody = { arguments ->
                                when (companion.type) {
                                    is RustType.Vec ->
                                        rustTemplate(
                                            """
                                            let mut v = $field.unwrap_or_default();
                                            v.push($arguments);
                                            $field = #{Some}(v);
                                            """,
                                            *scope,
                                        )

                                    is RustType.HashMap ->
                                        rustTemplate(
                                            """
                                            let mut hash_map = $field.unwrap_or_default();
                                            hash_map.insert($arguments);
                                            $field = #{Some}(hash_map);
                                            """,
                                            *scope,
                                        )

                                    else -> rustTemplate("$field = #{Some}($arguments);", *scope)
                                }
                            },
                            setterBody = { rust("$field = input;") },
                            getterBody = { rust("&$field") },
                        )
                    }

                is BuilderSection.MemberValueInBuild ->
                    companions.find { it.member.id == section.member.id }?.also { companion ->
                        rustTemplate(
                            "#{public_value}(self.${companion.name}.as_ref(), self.${companion.memberName})",
                            *scope,
                        )
                    }

                is BuilderSection.AdditionalFieldsInBuild ->
                    companions.forEach { companion ->
                        rustTemplate("${companion.name}: #{DocumentV2Slot}(self.${companion.name}),", *scope)
                    }

                is BuilderSection.AdditionalIntoBuilderSetters ->
                    companions.forEach { companion ->
                        rust(".${companion.setterName}(self.${companion.name}.0)")
                    }

                else -> {}
            }
        }
}

/**
 * Renders the fluent builder methods for [companion]. Each one delegates to the input builder's method of the same
 * name, which [DocumentV2Decorator] generates.
 */
internal fun RustWriter.renderDocumentV2FluentMethods(
    companion: DocumentV2Companion,
    runtimeConfig: RuntimeConfig,
) {
    renderDocumentV2Methods(
        companion,
        runtimeConfig,
        valueBody = { arguments -> rust("self.inner = self.inner.${companion.name}($arguments);") },
        setterBody = { rust("self.inner = self.inner.${companion.setterName}(input);") },
        getterBody = { rust("self.inner.${companion.getterName}()") },
    )
}

/**
 * Renders the value method, setter, and getter for [companion], with their docs and signatures. The body writers
 * write each method's body. [valueBody] gets the value method's arguments as a Rust argument list, and the setter's
 * argument is `input`. The value method and setter return `self` after their bodies.
 *
 * Like the `Document` methods, the value method appends an item for a list and inserts an entry for a map.
 */
private fun RustWriter.renderDocumentV2Methods(
    companion: DocumentV2Companion,
    runtimeConfig: RuntimeConfig,
    valueBody: RustWriter.(arguments: String) -> Unit,
    setterBody: RustWriter.() -> Unit,
    getterBody: RustWriter.() -> Unit,
) {
    val scope = documentV2Scope(runtimeConfig)
    val field = companion.unescapedMemberName
    val member = companion.member
    val v1Setter = member.setterName()
    val priority =
        """
        The `DocumentV2` form takes priority over a value set with `$field` or [`$v1Setter`](Self::$v1Setter).
        When it is set, the built value's `$field` field holds the same value with each `DocumentV2` converted by
        [`DocumentV2::to_document_lossy`](#{DocumentV2}::to_document_lossy).
        """.trimIndent()
    val required =
        "This field is required. Setting either form satisfies the requirement.".takeIf { member.isRequired }

    val (summary, parameters, arguments) =
        when (val type = companion.type) {
            is RustType.Vec -> {
                val input = type.member.asArgument("input")
                Triple(
                    """
                    Appends an item to the [`DocumentV2`](#{DocumentV2}) form of `$field`.

                    To override the contents of this collection use
                    [`${companion.setterName}`](Self::${companion.setterName}).
                    """.trimIndent(),
                    input.argument,
                    input.value,
                )
            }

            is RustType.HashMap -> {
                val k = type.key.asArgument("k")
                val v = type.member.asArgument("v")
                Triple(
                    """
                    Adds a key-value pair to the [`DocumentV2`](#{DocumentV2}) form of `$field`.

                    To override the contents of this collection use
                    [`${companion.setterName}`](Self::${companion.setterName}).
                    """.trimIndent(),
                    "${k.argument}, ${v.argument}",
                    "${k.value}, ${v.value}",
                )
            }

            else -> {
                val input = type.asArgument("input")
                Triple("Sets the [`DocumentV2`](#{DocumentV2}) form of `$field`.", input.argument, input.value)
            }
        }

    docsTemplate(listOfNotNull(summary, priority, required).joinToString("\n\n"), *scope)
    deprecatedShape(member)
    rustBlock("pub fn ${companion.name}(mut self, $parameters) -> Self") {
        valueBody(arguments)
        rust("self")
    }

    docsTemplate(
        """
        Sets or clears the [`DocumentV2`](#{DocumentV2}) form of `$field`.

        Clearing it with `None` lets a value set with `$field` or [`$v1Setter`](Self::$v1Setter) take effect again.
        """.trimIndent(),
        *scope,
    )
    deprecatedShape(member)
    rustBlockTemplate(
        "pub fn ${companion.setterName}(mut self, input: #{Option}<#{Type}>) -> Self",
        *scope,
        "Type" to companion.type,
    ) {
        setterBody()
        rust("self")
    }

    docsTemplate("Returns the [`DocumentV2`](#{DocumentV2}) form of `$field`, if one was set.", *scope)
    deprecatedShape(member)
    rustBlockTemplate(
        "pub fn ${companion.getterName}(&self) -> &#{Option}<#{Type}>",
        *scope,
        "Type" to companion.type,
    ) {
        getterBody()
    }
}
