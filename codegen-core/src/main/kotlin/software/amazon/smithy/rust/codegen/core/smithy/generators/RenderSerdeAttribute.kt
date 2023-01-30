package software.amazon.smithy.rust.codegen.core.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.util.isEventStream

public object RenderSerdeAttribute {
    public fun forStructureShape(writer: RustWriter, shape: StructureShape, model: Model) {
        if (shape.members().none { it.isEventStream(model) }) {
            this.writeAttributes(writer)
        }
    }

    public fun writeAttributes(writer: RustWriter) {
        writer.write("##[cfg_attr(aws_sdk_unstable, feature = \"serde-serialize\", serde::Serialize)]")
        writer.write("##[cfg_attr(aws_sdk_unstable, feature = \"serde-deserialize\", serde::Deserialize)]")
    }
}
