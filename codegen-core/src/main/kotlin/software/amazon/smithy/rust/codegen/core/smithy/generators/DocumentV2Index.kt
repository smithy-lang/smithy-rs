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
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.rustType

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
 * Finds the structure members that get a [DocumentV2Companion].
 *
 * A member gets one when it targets a document, or lists and maps whose elements are documents at any depth. Those
 * are the members whose Rust type holds `aws_smithy_types::Document` values directly, such as `Option<Document>`
 * or `Option<Vec<HashMap<String, Document>>>`.
 *
 * Other members that reach a document don't:
 * - A member that targets a structure, directly or through lists and maps. That structure holds its own companions.
 * - A union member. A union variant can't hold a hidden value, and adding a variant would change which variant
 *   existing code receives, so union variants stay `Document` only.
 */
class DocumentV2Index(model: Model) : KnowledgeIndex {
    private val membersWithCompanions: Map<ShapeId, List<MemberShape>> =
        model.structureShapes
            .associate { structure ->
                structure.id to structure.members().filter { holdsDocuments(model.expectShape(it.target), model) }
            }
            .filterValues { it.isNotEmpty() }

    /** Returns true when at least one of [shape]'s members gets a companion. */
    fun hasCompanions(shape: StructureShape): Boolean = membersWithCompanions.containsKey(shape.id)

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
                this is RustType.HashMap ->
                    RustType.HashMap(key, this.member.withDocumentV2(document, documentV2, member))
                else ->
                    throw CodegenException(
                        "`${member.id}` holds documents, but its Rust type contains `${render()}`, which " +
                            "`DocumentV2Index` doesn't know how to give a `DocumentV2` companion.",
                    )
            }
    }
}
