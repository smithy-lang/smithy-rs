package software.amazon.smithy.rust.codegen.smithy.protocols

import org.intellij.lang.annotations.Language
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.DocumentShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.TimestampShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.JsonNameTrait
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.lang.Custom
import software.amazon.smithy.rust.codegen.lang.RustMetadata
import software.amazon.smithy.rust.codegen.lang.RustType
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.contains
import software.amazon.smithy.rust.codegen.lang.render
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.stripOuter
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolMetadataProvider
import software.amazon.smithy.rust.codegen.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.util.dq

/**
 * JsonSerializerSymbolProvider annotates shapes and members with `serde` attributes
 */
class JsonSerializerSymbolProvider(
    private val model: Model,
    private val base: RustSymbolProvider,
    private val defaultTimestampFormat: TimestampFormatTrait.Format

) :
    SymbolMetadataProvider(base) {

    private fun MemberShape.serializedName() =
        this.getTrait(JsonNameTrait::class.java).map { it.value }.orElse(this.memberName)

    val httpIndex = HttpBindingIndex.of(model)
    val serializerBuilder = SerializerBuilder(base.config().runtimeConfig)
    override fun memberMeta(memberShape: MemberShape): RustMetadata {
        val currentMeta = base.toSymbol(memberShape).expectRustMetadata()
        val skipIfNone =
            if (base.toSymbol(memberShape).rustType().stripOuter<RustType.Reference>() is RustType.Option) {
                listOf(Custom("serde(skip_serializing_if = \"Option::is_none\")"))
            } else {
                listOf()
            }
        val renameAttribute = Custom("serde(rename = ${memberShape.serializedName().dq()})")
        val serializer = serializerFor(memberShape)
        val serdeAttribute = serializer?.let {
            listOf(Custom("serde(serialize_with = ${serializer.fullyQualifiedName().dq()})", listOf(it)))
        } ?: listOf()
        return currentMeta.copy(additionalAttributes = currentMeta.additionalAttributes + renameAttribute + serdeAttribute + skipIfNone)
    }

    override fun structureMeta(structureShape: StructureShape): RustMetadata {
        val currentMeta = base.toSymbol(structureShape).expectRustMetadata()
        return currentMeta.withDerive(RuntimeType.Serialize)
    }

    override fun unionMeta(unionShape: UnionShape): RustMetadata {
        val currentMeta = base.toSymbol(unionShape).expectRustMetadata()
        return currentMeta.withDerive(RuntimeType.Serialize)
    }

    override fun enumMeta(stringShape: StringShape): RustMetadata {
        val currentMeta = base.toSymbol(stringShape).expectRustMetadata()
        return currentMeta.withDerive(RuntimeType.Serialize)
    }

    private fun serializerFor(memberShape: MemberShape): RuntimeType? {
        val rustType = base.toSymbol(memberShape).rustType()
        val instant = base.toSymbol(TimestampShape.builder().id("dummy#ts").build()).rustType()
        val blob = base.toSymbol(BlobShape.builder().id("dummy#ts").build()).rustType()
        val document = base.toSymbol(DocumentShape.builder().id("dummy#ts").build()).rustType()
        if (!(rustType.contains(blob) || rustType.contains(instant) || rustType.contains(document))) {
            return null
        }
        val targetType = rustType.stripOuter<RustType.Reference>()
        val typeAsFunctionName = targetType.render().filter { it.isLetterOrDigit() }.toLowerCase()
        return when {
            rustType.contains(instant) -> instantSerializer(memberShape, typeAsFunctionName, targetType)
            rustType.contains(blob) -> blobSerializer(memberShape, typeAsFunctionName, targetType)
            rustType.contains(document) -> documentSerializer(memberShape, typeAsFunctionName, targetType)
            else -> null
        }
    }

    private fun documentSerializer(
        memberShape: MemberShape,
        typeAsFunctionName: String,
        argType: RustType
    ): RuntimeType {
        val symbol = base.toSymbol(memberShape)
        val fnName = typeAsFunctionName
        return RuntimeType.forInlineFun(fnName, "serde_util") { writer ->
            serializeFn(writer, fnName, symbol, argType) {
                write("todo!()")
            }
        }
    }

    private fun serializeFn(
        rustWriter: RustWriter,
        functionName: String,
        symbol: Symbol,
        targetType: RustType,
        body: RustWriter.() -> Unit
    ) {
        val ref = RustType.Reference(lifetime = null, value = targetType)
        val newSymbol = symbol.toBuilder().rustType(ref).build()
        rustWriter.rustBlock(
            "pub fn $functionName<S>(_inp: \$T, _serializer: S) -> " +
                "Result<<S as \$T>::Ok, <S as \$T>::Error> where S: \$T",
            newSymbol,
            RuntimeType.Serializer,
            RuntimeType.Serializer,
            RuntimeType.Serializer
        ) {
            body(this)
        }
    }

    private fun blobSerializer(memberShape: MemberShape, baseTypeName: String, argType: RustType): RuntimeType {
        val symbol = base.toSymbol(memberShape)
        val fnName = baseTypeName
        return RuntimeType.forInlineFun(fnName, "serde_util") { writer ->
            serializeFn(writer, fnName, symbol, argType) {
                serializerBuilder.render(this, baseTypeName)
            }
        }
    }

    private fun instantSerializer(memberShape: MemberShape, baseTypeName: String, argType: RustType): RuntimeType {
        val instantFormat =
            httpIndex.determineTimestampFormat(memberShape, HttpBinding.Location.PAYLOAD, defaultTimestampFormat)
        val symbol = base.toSymbol(memberShape)
        val fnName = "${baseTypeName}_${instantFormat.name.replace('-', '_').toLowerCase()}"
        return RuntimeType.forInlineFun(fnName, "serde_util") { rustWriter: RustWriter ->
            serializeFn(rustWriter, fnName, symbol, argType) {
                serializerBuilder.render(this, fnName)
            }
        }
    }
}

class SerializerBuilder(runtimeConfig: RuntimeConfig) {
    private val inp = "_inp"
    private val ser = "_serializer"
    private val HandWrittenSerializers: Map<String, (RustWriter) -> Unit> = mapOf(
        "optionblob" to { writer ->
            writer.rustBlock("match $inp") {
                write(
                    "Some(blob) => $ser.serialize_str(&\$T(blob.as_ref())),",
                    RuntimeType.Base64Encode(runtimeConfig)
                )
                write("None => $ser.serialize_none()")
            }
        },
        "blob" to { writer ->
            writer.write(
                "$ser.serialize_str(&\$T($inp.as_ref()))",
                RuntimeType.Base64Encode(runtimeConfig)
            )
        },

        "optioninstant_http_date" to { writer ->
            val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, TimestampFormatTrait.Format.HTTP_DATE)
            writer.rustBlock("match $inp") {
                write(
                    "Some(ts) => $ser.serialize_some(&ts.fmt(\$T)),", timestampFormatType
                )
                write("None => _serializer.serialize_none()")
            }
        },
        "optioninstant_date_time" to { writer ->
            val timestampFormatType = RuntimeType.TimestampFormat(runtimeConfig, TimestampFormatTrait.Format.DATE_TIME)
            writer.rustBlock("match $inp") {
                write(
                    "Some(ts) => $ser.serialize_some(&ts.fmt(\$T)),", timestampFormatType
                )
                write("None => _serializer.serialize_none()")
            }
        },
        "optioninstant_epoch_seconds" to { writer ->
            writer.rustBlock("match $inp") {
                write("Some(ts) => $ser.serialize_some(&ts.epoch_seconds()),")
                write("None => _serializer.serialize_none()")
            }
        },
        "instant_epoch_seconds" to { writer ->
            writer.write("$ser.serialize_i64($inp.epoch_seconds())")
        }
    )

    fun render(writer: RustWriter, name: String) =
        HandWrittenSerializers[name]?.also { it(writer) } ?: writer.write("todo!()")
}

@Language("Rust")
private fun String.rust(): String {
    return this
}
