package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.deserializeFunctionName
import software.amazon.smithy.rust.codegen.smithy.protocols.parse.JsonParserGenerator
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

class RestJsonDeserializerGenerator(
        protocolConfig: ProtocolConfig,
        private val httpBindingResolver: HttpBindingResolver,
) : JsonParserGenerator(protocolConfig, httpBindingResolver) {
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
            includedMembers: List<MemberShape>,
    ) {
        if (!renderedStructures.add(structureShape)) return
        val fnName = symbolProvider.deserializeFunctionName(structureShape)
        val unusedMut = if (includedMembers.isEmpty()) "##[allow(unused_mut)] " else ""
        writer.write("")
        writer.rustBlockTemplate(
                "pub fn $fnName(input: &[u8], ${unusedMut}mut builder: #{Builder}) -> Result<#{Builder}, #{Error}>",
                *codegenScope,
                "Builder" to structureShape.builderSymbol(symbolProvider),
        ) {
            rustTemplate(
                    """
                    let mut tokens_owned = #{json_token_iter}(#{or_empty}(input)).peekable();
                    let tokens = &mut tokens_owned;
                    #{expect_start_object}(tokens.next())?;
                """.trimIndent(),
                    *codegenScope
            )
            deserializeStructInner(includedMembers)
            expectEndOfTokenStream()
            rust("Ok(builder)")
        }
    }
}
