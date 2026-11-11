/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientProtocolLoader
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
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.DocumentV2Companion
import software.amazon.smithy.rust.codegen.core.smithy.generators.DocumentV2Index
import software.amazon.smithy.rust.codegen.core.smithy.generators.DocumentV2UnionVariant
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.shouldRedact

/**
 * Gives generated structures and ordinary client unions additive `DocumentV2` representations, found by
 * [DocumentV2Index], while keeping their existing `Document` API and default response behavior.
 *
 * Only clients that use schema serde ([SchemaSerdeAllowlist.usesSchemaSerdeExclusively]) get these APIs. Its
 * serializers understand both document forms. The `protocol_serde` serializers and parsers, which schema serde
 * replaces, use only `Document`, so other clients are generated as before.
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
 * - An ordinary union member that directly holds documents gets a second variant such as `JsonV2(DocumentV2)` and
 *   matching `as_json_v2`/`is_json_v2` methods. Both variants use the same Smithy member on the wire. Default
 *   deserialization continues to construct `Json(Document)` until the caller explicitly selects V2 decoding.
 *
 * Fluent builders get the same methods from [renderDocumentV2FluentMethods], which `FluentBuilderGenerator` calls.
 */
class DocumentV2Decorator : ClientCodegenDecorator {
    override val name: String = "DocumentV2"
    override val order: Byte = 0

    override fun symbolProvider(base: RustSymbolProvider): RustSymbolProvider {
        val context = base.moduleProviderContext
        val settings = context.settings as? ClientRustSettings ?: return base
        val service = context.serviceShape ?: return base
        return if (companionsEnabled(settings, base.model, service)) DocumentV2MetadataProvider(base) else base
    }

    override fun structureCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<StructureCustomization>,
    ): List<StructureCustomization> =
        if (documentV2CompanionsEnabled(codegenContext)) {
            baseCustomizations + DocumentV2StructureCustomization(codegenContext)
        } else {
            baseCustomizations
        }

    override fun builderCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<BuilderCustomization>,
    ): List<BuilderCustomization> =
        if (documentV2CompanionsEnabled(codegenContext)) {
            baseCustomizations + DocumentV2BuilderCustomization(codegenContext)
        } else {
            baseCustomizations
        }

    override fun unionCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<UnionCustomization>,
    ): List<UnionCustomization> =
        if (documentV2CompanionsEnabled(codegenContext)) {
            baseCustomizations + DocumentV2UnionCustomization(codegenContext)
        } else {
            baseCustomizations
        }

    /** Re-exports `DocumentUnionDecodeMode` from `crate::config` when an operation's fluent builder accepts it. */
    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val operations = TopDownIndex.of(codegenContext.model).getContainedOperations(codegenContext.serviceShape)
        if (operations.any { DocumentUnionDecodeMode.applies(codegenContext, it) }) {
            rustCrate.withModule(ClientRustModule.config) {
                rustTemplate("pub use #{Mode};", "Mode" to DocumentUnionDecodeMode.type(codegenContext.runtimeConfig))
            }
        }
    }
}

/** Whether [codegenContext]'s structures get `DocumentV2` companions. See [DocumentV2Decorator]. */
internal fun documentV2CompanionsEnabled(codegenContext: ClientCodegenContext): Boolean {
    val enabled = SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)
    check(enabled == companionsEnabled(codegenContext.settings, codegenContext.model, codegenContext.serviceShape)) {
        "`DocumentV2Decorator` decided whether `${codegenContext.serviceShape.id}` uses schema serde from the " +
            "default protocol choice, which isn't `${codegenContext.protocol}`. Its symbol provider and its " +
            "customizations would disagree about which structures have `DocumentV2` companions."
    }
    return enabled
}

/**
 * The companions of [shape]'s members, or none when [codegenContext]'s structures don't get companions. See
 * [DocumentV2Decorator].
 */
internal fun documentV2Companions(
    codegenContext: ClientCodegenContext,
    shape: StructureShape,
): List<DocumentV2Companion> =
    if (documentV2CompanionsEnabled(codegenContext)) {
        DocumentV2Index.of(codegenContext.model).companions(shape, codegenContext.symbolProvider)
    } else {
        emptyList()
    }

/**
 * [documentV2CompanionsEnabled] for the symbol provider, which exists before the codegen context. It picks the
 * protocol as `ClientCodegenVisitor` does, with the default protocols, and [documentV2CompanionsEnabled] checks that
 * the two agree.
 */
private fun companionsEnabled(
    settings: ClientRustSettings,
    model: Model,
    service: ServiceShape,
): Boolean {
    val serviceProtocols = ServiceIndex.of(model).getProtocols(service)
    val protocol = ClientProtocolLoader.DefaultProtocols.keys.firstOrNull { it in serviceProtocols }
    return SchemaSerdeAllowlist.usesSchemaSerdeExclusively(settings, service, protocol)
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

/** Adds the client-only V2 representation of each directly document-bearing union member. */
private class DocumentV2UnionCustomization(private val codegenContext: ClientCodegenContext) : UnionCustomization() {
    private val index = DocumentV2Index.of(codegenContext.model)
    private val scope =
        arrayOf(
            *preludeScope,
            "DocumentV2" to RuntimeType.documentV2(codegenContext.runtimeConfig),
        )

    override fun section(section: UnionSection): Writable =
        writable {
            val variants = index.unionVariants(section.shape, codegenContext.symbolProvider)
            when (section) {
                is UnionSection.AdditionalVariants -> variants.forEach { renderVariant(it) }
                is UnionSection.AdditionalMethods -> variants.forEach { renderMethods(it) }
                is UnionSection.AdditionalDebugArms -> variants.forEach { renderDebugArm(section, it) }
            }
        }

    private fun RustWriter.renderVariant(variant: DocumentV2UnionVariant) {
        docsTemplate(
            "Contains the [`DocumentV2`](#{DocumentV2}) representation of the modeled " +
                "`${variant.member.memberName}` member.",
            *scope,
        )
        deprecatedShape(variant.member)
        rustTemplate("${variant.variantName}(#{Type}),", "Type" to variant.type)
    }

    private fun RustWriter.renderMethods(variant: DocumentV2UnionVariant) {
        docsTemplate(
            "Tries to extract the [`DocumentV2`](#{DocumentV2}) representation of the modeled " +
                "`${variant.member.memberName}` member. Returns `Err(&Self)` for every other variant.",
            *scope,
        )
        deprecatedShape(variant.member)
        rustBlockTemplate(
            "pub fn as_${variant.accessorName}(&self) -> #{Result}<&#{Type}, &Self>",
            *preludeScope,
            "Type" to variant.type,
        ) {
            rustTemplate(
                "if let Self::${variant.variantName}(value) = self { #{Ok}(value) } else { #{Err}(self) }",
                *preludeScope,
            )
        }

        rust("/// Returns true if this is [`${variant.variantName}`](Self::${variant.variantName}).")
        deprecatedShape(variant.member)
        rustBlock("pub fn is_${variant.accessorName}(&self) -> bool") {
            rust("self.as_${variant.accessorName}().is_ok()")
        }
    }

    private fun RustWriter.renderDebugArm(
        section: UnionSection.AdditionalDebugArms,
        variant: DocumentV2UnionVariant,
    ) {
        val unionName = section.unionSymbol.name
        if (variant.member.shouldRedact(codegenContext.model)) {
            rust("$unionName::${variant.variantName}(_) => f.debug_tuple($REDACTION).finish(),")
        } else {
            rust(
                "$unionName::${variant.variantName}(value) => " +
                    "f.debug_tuple(${variant.variantName.dq()}).field(&value).finish(),",
            )
        }
    }
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
            /// Returns the [`DocumentV2`](#{DocumentV2}) form of `$field`, if there is one.
            ///
            /// A value read from a response has it, and so does a value built with the
            /// `DocumentV2` builder methods. When this returns `Some`, the `$field` field holds
            /// the same value with each `DocumentV2` converted by
            /// [`DocumentV2::to_document_lossy`](#{DocumentV2}::to_document_lossy).
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
