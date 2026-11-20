/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * Shares readable schema static names between serialization and deserialization.
 * Names are allocated in shape-ID order within each Rust module. Numeric suffixes
 * resolve collisions, including the nested `_KEY`, `_VALUE`, and `_MEMBER` statics.
 */
internal class ServerSchemaNames(
    private val model: Model,
    private val symbolProvider: RustSymbolProvider,
    shapes: Collection<Shape> = emptyList(),
) {
    internal class Names(
        val prefix: String,
        private val members: Map<ShapeId, String>,
    ) {
        fun getMemberName(memberId: ShapeId): String =
            members[memberId] ?: error("No schema static name allocated for member $memberId")
    }

    private val usedNames = mutableMapOf<String, MutableSet<String>>()
    private val namesByShape = mutableMapOf<Pair<ShapeId, String?>, Names>()

    init {
        shapes.filter { it is StructureShape || it is UnionShape }.sortedBy { it.id }.forEach { forShape(it) }
    }

    fun forShape(
        shape: Shape,
        prefixOverride: String? = null,
    ): Names =
        namesByShape.getOrPut(shape.id to prefixOverride) {
            val symbol = symbolProvider.toSymbol(shape)
            val used = usedNames.getOrPut(symbol.namespace) { mutableSetOf() }
            val prefix =
                allocate(prefixOverride ?: "SHAPE_${constantName(symbol.name)}", used) {
                    setOf("${it}_SCHEMA", "${it}_TRAITS")
                }
            val members =
                shape.members().sortedBy { it.id }.associate { member ->
                    val name =
                        allocate("${prefix}_MEMBER_${constantName(symbolProvider.toMemberName(member))}", used) {
                            buildSet {
                                add(it)
                                add("${it}_TRAITS")
                                aggregateNames(it, model.expectShape(member.target), emptySet(), this)
                            }
                        }
                    member.id to name
                }
            Names(prefix, members)
        }

    fun getMemberName(memberId: ShapeId): String {
        val member = model.expectShape(memberId, MemberShape::class.java)
        return forShape(model.expectShape(member.container)).getMemberName(memberId)
    }

    private fun allocate(
        preferredName: String,
        used: MutableSet<String>,
        names: (String) -> Set<String>,
    ): String {
        var suffix = 0
        while (true) {
            val candidate = if (suffix == 0) preferredName else "${preferredName}_$suffix"
            val generatedNames = names(candidate)
            if (generatedNames.none { it in used }) {
                used.addAll(generatedNames)
                return candidate
            }
            suffix += 1
        }
    }

    private fun aggregateNames(
        prefix: String,
        target: Shape,
        ancestors: Set<ShapeId>,
        names: MutableSet<String>,
    ) {
        // Invalid aggregate-only cycles are rejected by validateSchemaCollections.
        if (target.id in ancestors) return
        val members =
            when (target) {
                is MapShape -> listOf("KEY" to target.key, "VALUE" to target.value)
                is ListShape -> listOf("MEMBER" to target.member)
                else -> return
            }
        for ((suffix, member) in members) {
            val name = "${prefix}_$suffix"
            names.add(name)
            aggregateNames(name, model.expectShape(member.target), ancestors + target.id, names)
        }
    }

    private fun constantName(name: String): String = name.removePrefix("r#").toSnakeCase().uppercase()
}
