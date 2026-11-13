/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.KnowledgeIndex
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * The private `DocumentV2` companion of a structure member whose public field holds `Document` values.
 *
 * Names follow the member's existing accessors: a member whose field and builder method are `payload`, and whose
 * builder setter and getter are `set_payload` and `get_payload`, gets `payload_v2`, `set_payload_v2`, and
 * `get_payload_v2`.
 */
data class DocumentV2Companion(
    val member: MemberShape,
    /**
     * The public field's name from [RustSymbolProvider.toMemberName], such as `payload`. Raw identifiers are escaped
     * for writer templates, as in `r##type`.
     */
    val memberName: String,
    /**
     * The companion's value type: the public field's type without its outer `Option`, with `DocumentV2` in place of
     * `Document`. For example `DocumentV2` or `Vec<HashMap<String, DocumentV2>>`.
     */
    val type: RustType,
    /** The companion's field name, which is also the name of its structure getter and builder method. */
    val name: String,
    /** The builder setter that takes an `Option`. */
    val setterName: String,
    /** The builder getter. */
    val getterName: String,
) {
    /** [memberName] without a raw identifier prefix, for docs. */
    val unescapedMemberName: String get() = memberName.removePrefix("r##")
}

/**
 * An additive client union variant whose payload replaces every directly held `Document` with `DocumentV2`.
 *
 * The synthetic Rust variant is another representation of [member], not another Smithy member. Schema and protocol
 * serializers must therefore use [member]'s original wire name and schema index for both representations.
 */
data class DocumentV2UnionVariant(
    val member: MemberShape,
    /** The existing generated Rust variant, such as `Json`. */
    val originalVariantName: String,
    /** The additive Rust variant, such as `JsonV2`. */
    val variantName: String,
    /** The suffix shared by the additive `as_*` and `is_*` methods, such as `json_v2`. */
    val accessorName: String,
    /** The additive variant's payload type, such as `DocumentV2` or `Vec<DocumentV2>`. */
    val type: RustType,
)

/**
 * Finds generated structure companions and additive client union variants for modeled document values.
 *
 * Structure members get a companion when they target a document, or lists and maps whose elements are documents at
 * any depth. Those are the members whose Rust type holds `aws_smithy_types::Document` values directly, such as
 * `Option<Document>` or `Option<Vec<HashMap<String, Document>>>`.
 *
 * Union members with the same direct document/list/map forms get a [DocumentV2UnionVariant]. The synthetic Rust
 * variant is client-only and additive; it serializes as the original Smithy member. Deserialization keeps selecting
 * the original variant unless a caller explicitly opts into the V2 response mode.
 *
 * Members that reach a document through a structure or another union don't get a companion or synthetic leaf
 * variant. The nested aggregate owns its own representation.
 */
class DocumentV2Index(model: Model) : KnowledgeIndex {
    private val membersWithCompanions: Map<ShapeId, List<MemberShape>> =
        model.structureShapes
            .associate { structure ->
                structure.id to structure.members().filter { holdsDocuments(model.expectShape(it.target), model) }
            }
            .filterValues { it.isNotEmpty() }

    private val membersWithUnionVariants: Map<ShapeId, List<MemberShape>> =
        model.unionShapes
            .associate { union ->
                union.id to union.members().filter { holdsDocuments(model.expectShape(it.target), model) }
            }
            .filterValues { it.isNotEmpty() }

    private val walker = DirectedWalker(model)
    private val reachesUnionVariantsCache = mutableMapOf<ShapeId, Boolean>()

    /** Returns true when at least one of [shape]'s members gets a companion. */
    fun hasCompanions(shape: StructureShape): Boolean = membersWithCompanions.containsKey(shape.id)

    /** Returns true when at least one of [shape]'s members gets a [DocumentV2UnionVariant]. */
    fun hasUnionVariants(shape: UnionShape): Boolean = membersWithUnionVariants.containsKey(shape.id)

    /**
     * Returns true when reading [shape] can read a union that has a [DocumentV2UnionVariant], at any depth, including
     * [shape] itself. Reading such a shape depends on the `DocumentUnionDecodeMode`.
     */
    fun reachesUnionVariants(shape: Shape): Boolean =
        reachesUnionVariantsCache.getOrPut(shape.id) {
            walker.walkShapes(shape).any { it is UnionShape && hasUnionVariants(it) }
        }

    /**
     * Returns the companions of [shape]'s members, in member order.
     *
     * Throws a [CodegenException] when a companion's name is already the name of one of [shape]'s member accessors,
     * such as a `payload_v2` field next to a `payload` document.
     */
    fun companions(
        shape: StructureShape,
        symbolProvider: RustSymbolProvider,
    ): List<DocumentV2Companion> {
        val members = membersWithCompanions[shape.id] ?: return emptyList()
        val runtimeConfig = symbolProvider.config.runtimeConfig
        val document = RuntimeType.document(runtimeConfig).toSymbol().rustType()
        val documentV2 = RuntimeType.documentV2(runtimeConfig).toSymbol().rustType()

        val companions =
            members.map { member ->
                val memberName = symbolProvider.toMemberName(member)
                val v1Type = symbolProvider.toSymbol(member).rustType().stripOuter<RustType.Option>()
                DocumentV2Companion(
                    member = member,
                    memberName = memberName,
                    type = v1Type.withDocumentV2(document, documentV2, member),
                    name = "${memberName.removePrefix("r##")}_v2",
                    setterName = "${member.setterName()}_v2",
                    getterName = "${member.getterName()}_v2",
                )
            }

        val accessorOwners =
            shape.members().flatMap { member ->
                listOf(symbolProvider.toMemberName(member).removePrefix("r##"), member.setterName(), member.getterName())
                    .map { it to member }
            }.toMap()
        for (companion in companions) {
            for (name in listOf(companion.name, companion.setterName, companion.getterName)) {
                accessorOwners[name]?.also { owner ->
                    throw CodegenException(
                        "`${shape.id}` member `${companion.member.memberName}` needs a `DocumentV2` accessor named " +
                            "`$name`, but member `${owner.memberName}` already uses that name. Rename one of the " +
                            "members, or extend `DocumentV2Index` to choose a different name.",
                    )
                }
            }
        }
        return companions
    }

    /**
     * Returns the additive `DocumentV2` variants for [shape], ordered by their existing Rust variant names.
     *
     * Throws a [CodegenException] when a synthetic variant or accessor would collide with a modeled member. A
     * document-bearing event-stream union is also rejected: its public event receiver needs a separately designed
     * representation rather than another ordinary union variant.
     */
    fun unionVariants(
        shape: UnionShape,
        symbolProvider: RustSymbolProvider,
    ): List<DocumentV2UnionVariant> {
        val members = membersWithUnionVariants[shape.id] ?: return emptyList()
        if (shape.hasTrait<StreamingTrait>()) {
            throw CodegenException(
                "`${shape.id}` is an event-stream union with a member that directly holds documents. " +
                    "DocumentV2 variants are only supported in ordinary client unions.",
            )
        }

        val runtimeConfig = symbolProvider.config.runtimeConfig
        val document = RuntimeType.document(runtimeConfig).toSymbol().rustType()
        val documentV2 = RuntimeType.documentV2(runtimeConfig).toSymbol().rustType()
        val modeledVariantOwners = shape.members().associateBy { symbolProvider.toMemberName(it) }
        val modeledAccessorOwners = shape.members().associateBy { symbolProvider.toSymbol(it).name.toSnakeCase() }
        val syntheticVariantOwners = mutableMapOf<String, MemberShape>()
        val syntheticAccessorOwners = mutableMapOf<String, MemberShape>()

        return members
            .map { member ->
                val originalVariantName = symbolProvider.toMemberName(member)
                val variantName = "${originalVariantName.removePrefix("r##")}V2"
                val accessorName = "${symbolProvider.toSymbol(member).name.toSnakeCase()}_v2"

                fun collision(
                    generatedName: String,
                    kind: String,
                    owner: MemberShape?,
                ) {
                    if (owner != null) {
                        throw CodegenException(
                            "`${shape.id}` member `${member.memberName}` needs a DocumentV2 $kind named " +
                                "`$generatedName`, but member `${owner.memberName}` already uses that name. " +
                                "Rename one of the members, or extend `DocumentV2Index` to choose a different name.",
                        )
                    }
                }

                collision(variantName, "variant", modeledVariantOwners[variantName])
                collision(accessorName, "accessor", modeledAccessorOwners[accessorName])
                collision(variantName, "variant", syntheticVariantOwners.putIfAbsent(variantName, member))
                collision(accessorName, "accessor", syntheticAccessorOwners.putIfAbsent(accessorName, member))

                DocumentV2UnionVariant(
                    member = member,
                    originalVariantName = originalVariantName,
                    variantName = variantName,
                    accessorName = accessorName,
                    type =
                        symbolProvider
                            .toSymbol(member)
                            .rustType()
                            .stripOuter<RustType.Option>()
                            .withDocumentV2(document, documentV2, member),
                )
            }.sortedBy { it.originalVariantName }
    }

    companion object {
        fun of(model: Model): DocumentV2Index = model.getKnowledge(DocumentV2Index::class.java, ::DocumentV2Index)

        private fun holdsDocuments(
            target: Shape,
            model: Model,
        ): Boolean =
            when (target) {
                is DocumentShape -> true
                is CollectionShape -> holdsDocuments(model.expectShape(target.member.target), model)
                is MapShape -> holdsDocuments(model.expectShape(target.value.target), model)
                else -> false
            }

        private fun RustType.withDocumentV2(
            document: RustType,
            documentV2: RustType,
            member: MemberShape,
        ): RustType =
            when {
                this == document -> documentV2
                this is RustType.Option -> RustType.Option(this.member.withDocumentV2(document, documentV2, member))
                this is RustType.Vec -> RustType.Vec(this.member.withDocumentV2(document, documentV2, member))
                this is RustType.HashSet -> RustType.HashSet(this.member.withDocumentV2(document, documentV2, member))
                this is RustType.Box -> RustType.Box(this.member.withDocumentV2(document, documentV2, member))
                this is RustType.HashMap ->
                    RustType.HashMap(key, this.member.withDocumentV2(document, documentV2, member))
                else ->
                    throw CodegenException(
                        "`${member.id}` holds documents, but its Rust type contains `${render()}`, which " +
                            "`DocumentV2Index` doesn't know how to represent with `DocumentV2`.",
                    )
            }
    }
}
