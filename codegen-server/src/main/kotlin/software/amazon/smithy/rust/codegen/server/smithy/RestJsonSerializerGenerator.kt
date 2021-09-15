package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.serialize.JsonSerializerGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

class RestJsonServerSerializerGenerator(
        protocolConfig: ProtocolConfig,
        private val httpBindingResolver: HttpBindingResolver,
) : JsonSerializerGenerator(protocolConfig, httpBindingResolver) {
    private val renderedStructures = mutableSetOf<StructureShape>()

    fun render(writer: RustWriter, operationShape: OperationShape) {
        renderInput(writer, operationShape)
        renderOutput(writer, operationShape)
        renderErrors(writer, operationShape)
    }

    private fun renderInput(writer: RustWriter, operationShape: OperationShape) {
        val httpDocumentMembers =
                httpBindingResolver.requestMembers(operationShape, HttpLocation.DOCUMENT)
        renderStructure(writer, operationShape.inputShape(model), httpDocumentMembers)
    }

    private fun renderOutput(writer: RustWriter, operationShape: OperationShape) {
        val httpDocumentMembers =
                httpBindingResolver.responseMembers(operationShape, HttpLocation.DOCUMENT)
        renderStructure(writer, operationShape.outputShape(model), httpDocumentMembers)
    }

    private fun renderErrors(writer: RustWriter, operationShape: OperationShape) {
        operationShape.errors.forEach { error ->
            val errorShape = model.expectShape(error, StructureShape::class.java)
            renderStructure(writer, errorShape, errorShape.members().toList())
        }
    }

    private fun renderStructure(
            writer: RustWriter,
            structureShape: StructureShape,
            includedMembers: List<MemberShape>? = null,
    ) {
        if (!renderedStructures.add(structureShape)) return
        val fnName = symbolProvider.serializeFunctionName(structureShape)
        writer.write("")
        writer.rustBlockTemplate(
                "pub fn $fnName(value: &#{target}) -> Result<String, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(structureShape)
        ) {
            rust("let mut out = String::new();")
            rustTemplate("let mut object = #{JsonObjectWriter}::new(&mut out);", *codegenScope)
            serializeStructure(StructContext("object", "value", structureShape), includedMembers)
            rust("object.finish();")
            rust("Ok(out)")
        }
    }
}
