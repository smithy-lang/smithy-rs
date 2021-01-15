/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBinding
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.contains
import software.amazon.smithy.rust.codegen.rustlang.render
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.rustType

/**
 * Generate custom serialization and deserialization functions when required.
 *
 * The general structure is:
 *  For a given type that does not implement serialize/deserialize, convert it to a `newtype` that _does_ (for example,
 *  see `InstantEpoch` in `instant_epoch.rs`. Then, using those types, invoke the serde derived serializer.
 *
 *  The generated code isn't optimal performance-wise. It uses `.collect()` (creating a new Vector from an iterator)
 *  in places that may be avoidable.
 *  This may be an eventual performance bottleneck, but it should be totally avoidable with slightly more complex
 *  code generation. Furthermore, these code paths are only hit for custom types.
 */
class CustomSerializerGenerator(
    private val symbolProvider: RustSymbolProvider,
    private val model: Model,
    private val defaultTimestampFormat: TimestampFormatTrait.Format
) {
    private val inp = "_inp"
    private val ser = "_serializer"
    private val httpBindingIndex = HttpBindingIndex.of(model)
    private val runtimeConfig = symbolProvider.config().runtimeConfig

    private val instant = RuntimeType.Instant(runtimeConfig).toSymbol().rustType()
    private val blob = RuntimeType.Blob(runtimeConfig).toSymbol().rustType()
    private val document = RuntimeType.Document(runtimeConfig).toSymbol().rustType()
    private val customShapes = setOf(instant, blob, document)

    /**
     * Generate a custom serialization function for [memberShape], suitable to be used
     * in the serde annotation `serialize_with` (See [JsonSerializerSymbolProvider])
     *
     * The returned object is a RuntimeType, which generates and creates all necessary dependencies when used.
     *
     * If this shape does not require custom serialization, this function returns null.
     *
     * For Example, for `Option<Instant>` being serialized in Epoch seconds:
     * To make it more readable, I've manually removed the fully qualified types.
     *   ```rust
     *   pub fn stdoptionoptioninstant_epoch_seconds_ser<S>(
     *     _inp: &Option<Instant>,
     *     _serializer: S,
     *   ) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
     *   where S: Serializer, {
     *      use Serialize;
     *      let el = _inp;
     *      el.as_ref()
     *          .map(|el| instant_epoch::InstantEpoch(*el))
     *          .serialize(_serializer)
     *   }
     *  ```
     *
     */

    fun serializerFor(memberShape: MemberShape): RuntimeType? {
        val symbol = symbolProvider.toSymbol(memberShape)
        val rustType = symbol.rustType()
        if (customShapes.none { rustType.contains(it) }) {
            return null
        }
        val fnName = serializerName(rustType, memberShape, "ser")
        return RuntimeType.forInlineFun(fnName, "serde_util") { writer ->
            serializeFn(writer, fnName, symbol) {
                serializer(rustType, memberShape)
            }
        }
    }

    /**
     * Generate a custom deserialization function for [memberShape], suitable to be used
     * in the serde annotation `serialize_with` (See [JsonSerializerSymbolProvider])
     *
     * The returned object is a RuntimeType, which generates and creates all necessary dependencies when used.
     *
     * If this shape does not require custom serialization, this function returns null.
     *
     * For example, the deserializer for `Option<Instant>` when converted to epoch seconds:
     * To make it more readable, I've manually removed the fully qualified types.
     * ```rust
     * pub fn stdoptionoptioninstant_epoch_seconds_deser<'de, D>(
     * _deser: D,
     * ) -> Result<Option<Instant>, D::Error>
     * where
     * D: Deserializer<'de>,
     * {
     *     use ::serde::Deserialize;
     *     Ok(
     *         Option::<instant_epoch::InstantEpoch>::deserialize(_deser)?
     *         .map(|el| el.0),
     *     )
     * }
     * ```
     */

    fun deserializerFor(memberShape: MemberShape): RuntimeType? {
        val symbol = symbolProvider.toSymbol(memberShape)
        val rustType = symbol.rustType()
        if (customShapes.none { rustType.contains(it) }) {
            return null
        }
        val fnName = serializerName(rustType, memberShape, "deser")
        return RuntimeType.forInlineFun(fnName, "serde_util") { writer ->
            deserializeFn(writer, fnName, symbol) {
                if (rustType.contains(document)) {
                    write("todo!()")
                } else {
                    deserializer(rustType, memberShape)
                }
            }
        }
    }

    private fun rollSer(t: RustType, memberShape: MemberShape): Writable {
        return when (t) {
            is RustType.Option -> writable {
                withBlock("el.as_ref().map(|el|", ")") {
                    rollSer(t.member, memberShape)(this)
                }
            }
            is RustType.Vec -> writable {
                withBlock("el.iter().map(|el|", ").collect::<Vec<_>>()") {
                    rollSer(t.member, memberShape)(this)
                }
            }
            is RustType.HashMap -> writable {
                withBlock("el.iter().map(|(k,el)|(k, ", ")).collect::<${t.namespace}::${t.name}<_, _>>()") {
                    rollSer(t.member, memberShape)(this)
                }
            }
            is RustType.Box -> writable {
                // TODO: this only works for exterior boxes.
                withBlock("let el = el.as_ref();", "") {
                    rollSer(t.member, memberShape)(this)
                }
            }
            is RustType.Reference -> writable {
                rollSer(t.member, memberShape)(this)
            }
            else -> if (customShapes.contains(t)) {
                writable {
                    serdeType(t, memberShape, SerdeDirection.Serialize)(this)
                    if (t == instant) {
                        write("(*el)")
                    } else {
                        write("(el)")
                    }
                }
            } else {
                TODO("unsupported type $t")
            }
        }
    }

    private fun RustWriter.serializer(t: RustType, memberShape: MemberShape) {
        write("use #T;", RuntimeType.Serialize)
        write("let el = $inp;")
        rollSer(t, memberShape)(this)
        write(".serialize($ser)")
    }

    /**
     * Generate a deserializer for the given type dynamically, eg:
     * ```rust
     *  use ::serde::Deserialize;
     *  Ok(
     *      Option::<crate::instant_epoch::InstantEpoch>::deserialize(_deser)?
     *          .map(|el| el.0)
     *  )
     * ```
     *
     * It utilizes a newtype that defines the given serialization to access the serde serializer
     * then performs any necessary mapping / unmapping. This has a slight disadvantage in that
     * that wrapping structures like `Vec` may be allocated twice—I think we should be able to avoid
     * this eventually however.
     */
    private fun RustWriter.deserializer(t: RustType, memberShape: MemberShape) {
        write("use #T;", RuntimeType.Deserialize)
        withBlock("Ok(", ")") {
            serdeType(t, memberShape, SerdeDirection.Deserialize)(this)
            write("::deserialize(_deser)?")
            unrollDeser(t)
        }
    }

    private fun RustWriter.unrollDeser(realType: RustType) {
        when (realType) {
            is RustType.Vec -> withBlock(".into_iter().map(|el|el", ").collect()") {
                unrollDeser(realType.member)
            }
            is RustType.Option -> withBlock(".map(|el|el", ")") {
                unrollDeser(realType.member)
            }

            is RustType.HashMap -> withBlock(".into_iter().map(|(k,el)|(k, el", ")).collect()") {
                unrollDeser(realType.member)
            }

            // We will only create HashSets of strings, so we shouldn't ever hit this
            is RustType.HashSet -> TODO("https://github.com/awslabs/smithy-rs/issues/44")

            is RustType.Box -> {
                unrollDeser(realType.member)
                write(".into()")
            }

            else -> if (customShapes.contains(realType)) {
                write(".0")
            } else {
                TODO("unsupported type $realType")
            }
        }
    }

    private fun RustWriter.serdeContainerType(realType: RustType.Container, memberShape: MemberShape) {
        val prefix = when (realType) {
            is RustType.HashMap -> "${realType.namespace}::${realType.name}::<String, "
            else -> "${realType.namespace}::${realType.name}::<"
        }
        withBlock(prefix, ">") {
            serdeType(realType.member, memberShape, SerdeDirection.Deserialize)(this)
        }
    }

    enum class SerdeDirection { Serialize, Deserialize }

    private fun serdeType(realType: RustType, memberShape: MemberShape, serdeDirection: SerdeDirection): Writable {
        return when (realType) {
            instant -> writable {
                val format = tsFormat(memberShape)
                when (format) {
                    TimestampFormatTrait.Format.DATE_TIME -> write("#T::InstantIso8601", RuntimeType.Instant8601)
                    TimestampFormatTrait.Format.EPOCH_SECONDS -> write("#T::InstantEpoch", RuntimeType.InstantEpoch)
                    TimestampFormatTrait.Format.HTTP_DATE -> write(
                        "#T::InstantHttpDate",
                        RuntimeType.InstantHttpDate
                    )
                    else -> write("todo!() /* unknown timestamp format */")
                }
            }
            blob -> writable {
                if (serdeDirection == SerdeDirection.Deserialize) {
                    write("#T::BlobDeser", RuntimeType.BlobSerde(runtimeConfig))
                } else {
                    write("#T::BlobSer", RuntimeType.BlobSerde(runtimeConfig))
                }
            }
            document -> writable {
                when (serdeDirection) {
                    SerdeDirection.Serialize -> write("#T::SerDoc", RuntimeType.DocJson)
                    SerdeDirection.Deserialize -> TODO("doc deserialization is not supported")
                }
            }
            is RustType.Container -> writable { serdeContainerType(realType, memberShape) }
            else -> TODO("$serdeDirection for $realType is not supported")
        }
    }

    /** correct argument type for the serde custom serializer */
    private fun serializerType(symbol: Symbol): Symbol {
        val unref = symbol.rustType().stripOuter<RustType.Reference>()

        // Convert `Vec<T>` to `[T]` when present. This is needed to avoid
        // Clippy complaining (and is also better in general).
        val outType = when (unref) {
            is RustType.Vec -> RustType.Slice(unref.member)
            else -> unref
        }
        val referenced = RustType.Reference(member = outType, lifetime = null)
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
}
