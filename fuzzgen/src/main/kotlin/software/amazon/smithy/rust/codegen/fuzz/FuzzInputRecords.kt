/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeType
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isTargetUnit
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained

/**
 * Local fuzz comparison records are intentionally unredacted. Never send these values to a
 * production logger: they retain every sensitive field so equality cannot hide a serde mismatch.
 * Debug is used only on primitive strings/bytes and canonical vectors, never on modeled shapes.
 */
internal class FuzzInputRecords(
    private val model: Model,
    private val symbols: RustSymbolProvider,
    private val document: RuntimeType,
    private val number: RuntimeType,
) {
    private val shapes = linkedMapOf<Shape, String>()

    fun record(
        shape: Shape,
        reference: String,
    ): String {
        val name = shapes.getOrPut(shape) { "record_input_${shapes.size}" }
        return "$name($reference)"
    }

    fun member(
        member: MemberShape,
        reference: String,
    ): String {
        val target = model.expectShape(member.target)
        return if (symbols.toSymbol(member).isOptional()) {
            "match $reference { @FuzzSome@(value) => format!(\"some:{}\", ${record(target, "value")}), @FuzzNone@ => \"none\".to_owned() }"
        } else {
            record(target, reference)
        }
    }

    /** Escape SDK raw identifiers before introducing RustWriter formatting placeholders. */
    fun template(expression: String): String =
        listOf("Vec", "String", "Some", "None", "Ok", "Err").fold(expression.replace("#", "##")) { text, name ->
            text.replace("@Fuzz$name@", "#{$name}")
        }

    fun render(writer: RustWriter) {
        // Rendering a container discovers its children. Keep iterating until the closure is emitted.
        var emitted = 0
        while (emitted < shapes.size) {
            val (shape, name) = shapes.entries.elementAt(emitted++)
            val argument =
                if (shape is StructureShape && shape.members().all { it.isEventStream(model) }) "_value" else "value"
            writer.rustTemplate(
                "fn $name($argument: &#{Type}) -> #{String} { #{Body} }",
                "Type" to symbols.toSymbol(shape),
                "Body" to
                    software.amazon.smithy.rust.codegen.core.rustlang.writable {
                        rustTemplate(template(expression(shape)), *preludeScope)
                    },
                *preludeScope,
            )
        }
        if (shapes.keys.any { it.type == ShapeType.DOCUMENT }) {
            writer.rustTemplate(
                """
                fn record_document(value: &#{Document}) -> #{String} {
                    match value {
                        #{Document}::Object(values) => {
                            let mut entries: #{Vec}<_> = values.iter()
                                .map(|(key, value)| (key.as_str(), record_document(value))).collect();
                            entries.sort_by(|left, right| left.0.cmp(right.0));
                            format!("object:{:?}", entries)
                        }
                        #{Document}::Array(values) => format!("array:{:?}", values.iter().map(record_document).collect::<#{Vec}<_>>()),
                        #{Document}::String(value) => format!("string:{:?}", value.as_str()),
                        #{Document}::Bool(value) => format!("bool:{}", value),
                        #{Document}::Null => "null".to_owned(),
                        #{Document}::Number(#{Number}::PosInt(value)) => format!("pos:{}", value),
                        #{Document}::Number(#{Number}::NegInt(value)) => format!("neg:{}", value),
                        #{Document}::Number(#{Number}::Float(value)) => format!("float:{:?}", value),
                    }
                }
                """,
                "Document" to document,
                "Number" to number,
                *preludeScope,
            )
        }
    }

    private fun expression(shape: Shape): String {
        val inner = if (shape.isDirectlyConstrained(symbols)) "value.inner()" else "value"
        return when (shape.type) {
            ShapeType.STRUCTURE -> {
                val fields =
                    (shape as StructureShape).members().filterNot { it.isEventStream(model) }.map {
                        val field = symbols.toMemberName(it)
                        "(${it.memberName.dq()}, ${member(it, "&value.$field")})"
                    }
                if (fields.isEmpty()) {
                    "\"structure:[]\".to_owned()"
                } else {
                    "format!(\"structure:{:?}\", [${fields.joinToString(",")}])"
                }
            }
            ShapeType.UNION -> {
                val arms =
                    (shape as UnionShape).members().joinToString("\n") {
                        val variant = symbols.toMemberName(it)
                        val name = symbols.toSymbol(shape).name
                        if (it.isTargetUnit()) {
                            "$name::$variant => ${("union:${it.memberName}:unit").dq()}.to_owned(),"
                        } else {
                            "$name::$variant(value) => format!(${("union:${it.memberName}:{}").dq()}, ${record(model.expectShape(it.target), "value")}),"
                        }
                    }
                // Symbol paths belong to the generated crate, not the local fuzz crate.
                val type = symbols.toSymbol(shape).rustType().render()
                "match value { ${arms.replace(symbols.toSymbol(shape).name + "::", "$type::")} }"
            }
            ShapeType.LIST, ShapeType.SET ->
                "format!(\"list:{:?}\", $inner.iter().map(|value| ${member(shape.allMembers.getValue("member"), "value")}).collect::<@FuzzVec@<_>>())"
            ShapeType.MAP -> {
                val key = record(model.expectShape(shape.allMembers.getValue("key").target), "key")
                val value = member(shape.allMembers.getValue("value"), "value")
                "{ let mut entries: @FuzzVec@<_> = $inner.iter().map(|(key, value)| ($key, $value)).collect(); entries.sort(); format!(\"map:{:?}\", entries) }"
            }
            ShapeType.STRING, ShapeType.ENUM -> "format!(\"string:{:?}\", value.as_str())"
            ShapeType.BLOB -> "format!(\"blob:{:?}\", $inner.as_ref() as &[u8])"
            ShapeType.TIMESTAMP -> "format!(\"timestamp:{}:{}\", value.secs(), value.subsec_nanos())"
            ShapeType.DOCUMENT -> "record_document(value)"
            ShapeType.FLOAT, ShapeType.DOUBLE ->
                "if $inner.is_nan() { \"float:NaN\".to_owned() } else { format!(\"float:{}\", $inner.to_bits()) }"
            ShapeType.BOOLEAN -> "format!(\"bool:{}\", value)"
            else -> "format!(\"number:{}\", $inner)"
        }
    }
}
