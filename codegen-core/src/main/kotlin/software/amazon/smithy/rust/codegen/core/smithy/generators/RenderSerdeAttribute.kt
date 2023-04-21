/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isStreaming

// Part of RFC30
public object RenderSerdeAttribute {
    public fun forStructureShape(writer: RustWriter, shape: StructureShape, model: Model) {
        if (shape.members().none { it.isEventStream(model) }) {
            writeAttributes(writer)
        }
    }

    public fun forBuilders(writer: RustWriter, shape: StructureShape, model: Model) {
        if (shape.members().none { it.isEventStream(model) }) {
            writeAttributes(writer)
        }
    }

    public fun skipIfStream(writer: RustWriter, member: MemberShape, model: Model) {
        if (member.isEventStream(model)) {
            return
        }
        if (member.isStreaming(model)) {
            Attribute("").SerdeSkip().render(writer)
        }
    }

    public fun importSerde(writer: RustWriter) {
        // we need this for skip serde to work
        Attribute.AllowUnusedImports.render(writer)
        Attribute("").SerdeSerializeOrDeserialize().render(writer)
        writer.raw("use serde;")
    }

    public fun writeAttributes(writer: RustWriter) {
        Attribute("").SerdeSerialize().render(writer)
        Attribute("").SerdeDeserialize().render(writer)
    }
}
