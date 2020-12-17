/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.lang.Attribute
import software.amazon.smithy.rust.codegen.lang.Custom
import software.amazon.smithy.rust.codegen.lang.RustMetadata
import software.amazon.smithy.rust.codegen.lang.RustType
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.contains
import software.amazon.smithy.rust.codegen.lang.render
import software.amazon.smithy.rust.codegen.lang.rust
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.rustTemplate
import software.amazon.smithy.rust.codegen.lang.stripOuter
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.isOptional
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.traits.InputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.OutputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticOutputTrait
import software.amazon.smithy.rust.codegen.util.dq

/**
 * JsonSerializerSymbolProvider annotates shapes and members with `serde` attributes
 */
class JsonSerializerSymbolProvider(
    private val model: Model,
    private val base: RustSymbolProvider,
    defaultTimestampFormat: TimestampFormatTrait.Format
) :
    SymbolMetadataProvider(base) {

    data class SerdeConfig(val serialize: Boolean, val deserialize: Boolean)

    private fun MemberShape.serializedName() =
        this.getTrait(JsonNameTrait::class.java).map { it.value }.orElse(this.memberName)

    private val serializerBuilder = SerializerBuilder(base, model, defaultTimestampFormat)
    override fun memberMeta(memberShape: MemberShape): RustMetadata {
        val currentMeta = base.toSymbol(memberShape).expectRustMetadata()
        val serdeConfig = serdeRequired(model.expectShape(memberShape.container))
        val attribs = mutableListOf<Attribute>()
        if (serdeConfig.serialize || serdeConfig.deserialize) {
            attribs.add(Custom("serde(rename = ${memberShape.serializedName().dq()})"))
        }
        if (serdeConfig.serialize) {
            if (base.toSymbol(memberShape).rustType().stripOuter<RustType.Reference>() is RustType.Option) {
                attribs.add(Custom("serde(skip_serializing_if = \"Option::is_none\")"))
            }
            serializerBuilder.serializerFor(memberShape)?.also {
                attribs.add(Custom("serde(serialize_with = ${it.fullyQualifiedName().dq()})", listOf(it)))
            }
        }
        if (serdeConfig.deserialize) {
            serializerBuilder.deserializerFor(memberShape)?.also {
                attribs.add(Custom("serde(deserialize_with = ${it.fullyQualifiedName().dq()})", listOf(it)))
            }
            if (model.expectShape(memberShape.container) is StructureShape && base.toSymbol(memberShape).isOptional()
            ) {
                attribs.add(Custom("serde(default)"))
            }
        }
        return currentMeta.copy(additionalAttributes = currentMeta.additionalAttributes + attribs)
    }

    override fun structureMeta(structureShape: StructureShape): RustMetadata = containerMeta(structureShape)
    override fun unionMeta(unionShape: UnionShape): RustMetadata = containerMeta(unionShape)
    override fun enumMeta(stringShape: StringShape): RustMetadata = containerMeta(stringShape)

    private fun containerMeta(container: Shape): RustMetadata {
        val currentMeta = base.toSymbol(container).expectRustMetadata()
        val requiredSerde = serdeRequired(container)
        return currentMeta
            .letIf(requiredSerde.serialize) { it.withDerives(RuntimeType.Serialize) }
            .letIf(requiredSerde.deserialize) { it.withDerives(RuntimeType.Deserialize) }
    }

    private fun serdeRequired(shape: Shape): SerdeConfig {
        return when {
            shape.hasTrait(InputBodyTrait::class.java) -> SerdeConfig(serialize = true, deserialize = false)
            shape.hasTrait(OutputBodyTrait::class.java) -> SerdeConfig(serialize = false, deserialize = true)

            // The bodies must be serializable. The top level inputs are _not_
            shape.hasTrait(SyntheticInputTrait::class.java) -> SerdeConfig(serialize = false, deserialize = false)
            shape.hasTrait(SyntheticOutputTrait::class.java) -> SerdeConfig(serialize = false, deserialize = false)
            else -> SerdeConfig(serialize = true, deserialize = true)
        }
    }
}

class SerializerBuilder(
    private val symbolProvider: RustSymbolProvider,
    model: Model,
    private val defaultTimestampFormat: TimestampFormatTrait.Format
) {
    private val inp = "_inp"
    private val ser = "_serializer"
    private val httpBindingIndex = HttpBindingIndex.of(model)
    private val runtimeConfig = symbolProvider.config().runtimeConfig

    // Small hack to get the Rust type for these problematic shapes
    private val instant = symbolProvider.toSymbol(TimestampShape.builder().id("dummy#ts").build()).rustType()
    private val blob = symbolProvider.toSymbol(BlobShape.builder().id("dummy#blob").build()).rustType()
    private val document = symbolProvider.toSymbol(DocumentShape.builder().id("dummy#doc").build()).rustType()
    private val customShapes = setOf(instant, blob, document)

    private val handWrittenSerializers: Map<String, (RustWriter) -> Unit> = mapOf(
        "stdoptionoptionblob_ser" to { writer ->
            writer.rustBlock("match $inp") {
                write(
                    "Some(blob) => $ser.serialize_str(&#T(blob.as_ref())),",
                    RuntimeType.Base64Encode(runtimeConfig)
                )
                write("None => $ser.serialize_none()")
            }
        },
        "blob_ser" to { writer ->
            writer.write(
                "$ser.serialize_str(&#T($inp.as_ref()))",
                RuntimeType.Base64Encode(runtimeConfig)
            )
        },

        "stdoptionoptioninstant_http_date_ser" to { writer ->
            val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, TimestampFormatTrait.Format.HTTP_DATE)
            writer.rustBlock("match $inp") {
                write(
                    "Some(ts) => $ser.serialize_some(&ts.fmt(#T)),", timestampFormatType
                )
                write("None => _serializer.serialize_none()")
            }
        },
        "stdoptionoptioninstant_date_time_ser" to { writer ->
            val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, TimestampFormatTrait.Format.DATE_TIME)
            writer.rustBlock("match $inp") {
                write(
                    "Some(ts) => $ser.serialize_some(&ts.fmt(#T)),", timestampFormatType
                )
                write("None => _serializer.serialize_none()")
            }
        },
        "stdoptionoptioninstant_epoch_seconds_ser" to { writer ->
            writer.rustBlock("match $inp") {
                write("Some(ts) => $ser.serialize_some(&ts.epoch_seconds()),")
                write("None => _serializer.serialize_none()")
            }
        },
        "instant_epoch_seconds_ser" to { writer ->
            writer.write("$ser.serialize_i64($inp.epoch_seconds())")
        },
        "document_ser" to { writer ->
            writer.write("use #T;", RuntimeType.Serialize)
            writer.write("#T::SerDoc($inp).serialize($ser)", RuntimeType.DocJson)
        }
    )

    // TODO: this whole thing needs to be overhauled to be composable
    private val handWrittenDeserializers: Map<String, (RustWriter) -> Unit> = mapOf(
        "stdoptionoptioninstant_epoch_seconds_deser" to { writer ->
            // Needed to pull the Option deserializer into scope
            writer.write("use #T;", RuntimeType.Deserialize)
            writer.rust(
                """
                let ts_opt = Option::<f64>::deserialize(_deser)?;
                Ok(ts_opt.map(| ts | Instant ::from_fractional_seconds(ts.floor() as i64, ts - ts.floor())))
            """
            )
        },
        "blob_deser" to { writer ->
            writer.rustTemplate(
                """
                use #{deserialize};
                use #{de}::Error;
                let data = <&str>::deserialize(_deser)?;
                #{base64_decode}(data)
                    .map(Blob::new)
                    .map_err(|_|D::Error::invalid_value(#{de}::Unexpected::Str(data), &"valid base64"))

            """,
                "deserialize" to RuntimeType.Deserialize,
                "de" to RuntimeType.Serde("de"),
                "base64_decode" to RuntimeType.Base64Decode(runtimeConfig)
            )
        },
        "stdoptionoptionblob_deser" to { writer ->
            writer.rustTemplate(
                """
                use #{deserialize};
                use #{de}::Error;
                Option::<&str>::deserialize(_deser)?.map(|data| {
                    #{base64_decode}(data)
                        .map(Blob::new)
                        .map_err(|_|D::Error::invalid_value(#{de}::Unexpected::Str(data), &"valid base64"))
                }).transpose()

            """,
                "deserialize" to RuntimeType.Deserialize,
                "de" to RuntimeType.Serde("de"),
                "base64_decode" to RuntimeType.Base64Decode(runtimeConfig)
            )
        },
        "instant_epoch_seconds_deser" to { writer ->
            writer.write("use #T;", RuntimeType.Deserialize)
            writer.rust(
                """
                let ts = f64::deserialize(_deser)?;
                Ok(Instant::from_fractional_seconds(ts.floor() as i64, ts - ts.floor()))
            """
            )
        }

    )

    /** correct argument type for the serde custom serializer */
    private fun serializerType(symbol: Symbol): Symbol {
        val unref = symbol.rustType().stripOuter<RustType.Reference>()

        // Convert `Vec<T>` to `[T]` when present. This is needed to avoid
        // Clippy complaining (and is also better in general).
        val outType = when (unref) {
            is RustType.Vec -> RustType.Slice(unref.member)
            else -> unref
        }
        val referenced = RustType.Reference(value = outType, lifetime = null)
        return symbol.toBuilder().rustType(referenced).build()
    }

    private fun tsFormat(memberShape: MemberShape) =
        httpBindingIndex.determineTimestampFormat(memberShape, HttpBinding.Location.PAYLOAD, defaultTimestampFormat)

    private fun serializerName(rustType: RustType, memberShape: MemberShape, suffix: String): String {
        val context = when {
            rustType.contains(instant) -> tsFormat(memberShape).name.replace('-', '_').toLowerCase()
            else -> null
        }
        val typeToFnName =
            rustType.stripOuter<RustType.Reference>().render(fullyQualified = true).filter { it.isLetterOrDigit() }
                .toLowerCase()
        return listOfNotNull(typeToFnName, context, suffix).joinToString("_")
    }

    private fun serializeFn(
        rustWriter: RustWriter,
        functionName: String,
        symbol: Symbol,
        body: RustWriter.() -> Unit
    ) {
        rustWriter.rustBlock(
            "pub fn $functionName<S>(_inp: #1T, _serializer: S) -> " +
                "Result<<S as #2T>::Ok, <S as #2T>::Error> where S: #2T",
            serializerType(symbol),
            RuntimeType.Serializer
        ) {
            body(this)
        }
    }

    private fun deserializeFn(
        rustWriter: RustWriter,
        functionName: String,
        symbol: Symbol,
        body: RustWriter.() -> Unit
    ) {
        rustWriter.rustBlock(
            "pub fn $functionName<'de, D>(_deser: D) -> Result<#T, D::Error> where D: #T<'de>",
            symbol,
            RuntimeType.Deserializer
        ) {
            body(this)
        }
    }

    fun serializerFor(memberShape: MemberShape): RuntimeType? {
        val symbol = symbolProvider.toSymbol(memberShape)
        val rustType = symbol.rustType()
        if (customShapes.none { rustType.contains(it) }) {
            return null
        }
        val fnName = serializerName(rustType, memberShape, "ser")
        return RuntimeType.forInlineFun(fnName, "serde_util") { writer ->
            serializeFn(writer, fnName, symbol) {
                handWrittenSerializers[fnName]?.also { it(this) } ?: write("todo!()")
            }
        }
    }

    fun deserializerFor(memberShape: MemberShape): RuntimeType? {
        val symbol = symbolProvider.toSymbol(memberShape)
        val rustType = symbol.rustType()
        if (customShapes.none { rustType.contains(it) }) {
            return null
        }
        val fnName = serializerName(rustType, memberShape, "deser")
        return RuntimeType.forInlineFun(fnName, "serde_util") { writer ->
            deserializeFn(writer, fnName, symbol) {
                handWrittenDeserializers[fnName]?.also { it(this) } ?: write("todo!()")
            }
        }
    }
}
