/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.traits.InputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.OutputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait

/**
 * JsonSerializerSymbolProvider annotates shapes and members with `serde` attributes
 */
class JsonSerializerSymbolProvider(
    private val model: Model,
    private val base: RustSymbolProvider,
    defaultTimestampFormat: TimestampFormatTrait.Format
) :
    SymbolMetadataProvider(base) {

    data class SerdeConfig(val deserialize: Boolean)

    private fun MemberShape.serializedName() = this.getTrait<JsonNameTrait>()?.value ?: this.memberName

    private val serializerBuilder = CustomSerializerGenerator(base, model, defaultTimestampFormat)
    override fun memberMeta(memberShape: MemberShape): RustMetadata {
        val currentMeta = base.toSymbol(memberShape).expectRustMetadata()
        val serdeConfig = serdeRequired(model.expectShape(memberShape.container))
        val attribs = mutableListOf<Attribute>()
        if (serdeConfig.deserialize) {
            attribs.add(Attribute.Custom("serde(rename = ${memberShape.serializedName().dq()})"))
        }
        if (serdeConfig.deserialize) {
            serializerBuilder.deserializerFor(memberShape)?.also {
                attribs.add(Attribute.Custom("serde(deserialize_with = ${it.fullyQualifiedName().dq()})", listOf(it)))
            }
            if (model.expectShape(memberShape.container) is StructureShape && base.toSymbol(memberShape)
                .isOptional()
            ) {
                attribs.add(Attribute.Custom("serde(default)"))
            }
        }
        return currentMeta.copy(additionalAttributes = currentMeta.additionalAttributes + attribs)
    }

    override fun structureMeta(structureShape: StructureShape): RustMetadata = containerMeta(structureShape)
    override fun unionMeta(unionShape: UnionShape): RustMetadata = containerMeta(unionShape)
    override fun enumMeta(stringShape: StringShape): RustMetadata = base.toSymbol(stringShape).expectRustMetadata()

    private fun containerMeta(container: Shape): RustMetadata {
        val currentMeta = base.toSymbol(container).expectRustMetadata()
        val requiredSerde = serdeRequired(container)
        return currentMeta.letIf(requiredSerde.deserialize) { it.withDerives(RuntimeType.Deserialize) }
    }

    private fun serdeRequired(shape: Shape): SerdeConfig {
        return when {
            shape.hasTrait<InputBodyTrait>() -> SerdeConfig(deserialize = false)
            shape.hasTrait<OutputBodyTrait>() -> SerdeConfig(deserialize = true)

            // The bodies must be serializable. The top level inputs are _not_
            shape.hasTrait<SyntheticInputTrait>() -> SerdeConfig(deserialize = false)
            shape.hasTrait<SyntheticOutputTrait>() -> SerdeConfig(deserialize = false)
            else -> SerdeConfig(deserialize = true)
        }
    }
}
