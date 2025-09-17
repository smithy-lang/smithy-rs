package software.amazon.smithy.rust.codegen.server.traits

import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.AbstractTrait
import software.amazon.smithy.model.traits.Trait

class ValidationFieldListTrait(sourceLocation: SourceLocation) : AbstractTrait(ID, sourceLocation) {
    companion object {
        val ID: ShapeId = ShapeId.from("smithy.rust.codegen.server.traits#validationFieldList")
    }

    override fun createNode(): Node = Node.objectNode()

    class Provider : AbstractTrait.Provider(ID) {
        override fun createTrait(
            target: ShapeId,
            value: Node,
        ): Trait {
            val result = ValidationFieldListTrait(value.sourceLocation)
            result.setNodeCache(value)
            return result
        }
    }
}
