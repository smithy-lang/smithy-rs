package software.amazon.smithy.rust.codegen.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.streamingMember
import software.amazon.smithy.rust.codegen.util.outputShape

/** Event streams are not supported yet */
object EventStreamRemover {
    fun transform(model: Model) = ModelTransformer.create().filterShapes(model) { shape ->
        if (shape !is OperationShape) {
            true
        } else {
            shape.outputShape(model).streamingMember(model)?.isUnionShape ?: true
        }
    }
}
