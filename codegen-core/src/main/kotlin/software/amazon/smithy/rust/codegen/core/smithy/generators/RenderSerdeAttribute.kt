/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.util.isEventStream

public object RenderSerdeAttribute {
    public fun forStructureShape(writer: RustWriter, shape: StructureShape, model: Model) {
        if (shape.members().none { it.isEventStream(model) }) {
            writeAttributes(writer)
        }
    }

    public fun writeAttributes(writer: RustWriter) {
        Attribute("").SerdeSerialize().render(writer)
        Attribute("").SerdeDeserialize().render(writer)
    }
}
