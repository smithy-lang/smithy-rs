package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.protocols.serializeFunctionName
import software.amazon.smithy.rust.codegen.util.inputShape

class AwsQuerySerializerGenerator(protocolConfig: ProtocolConfig) : StructuredDataSerializerGenerator {
    private val model = protocolConfig.model
    private val symbolProvider = protocolConfig.symbolProvider
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val serializerError = RuntimeType.SerdeJson("error::Error")
    private val codegenScope = arrayOf(
        "String" to RuntimeType.String,
        "Error" to serializerError,
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
    )

    override fun payloadSerializer(member: MemberShape): RuntimeType {
        val fnName = symbolProvider.serializeFunctionName(member)
        val target = model.expectShape(member.target, StructureShape::class.java)
        return RuntimeType.forInlineFun(fnName, "operation_ser") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope,
                "target" to symbolProvider.toSymbol(target)
            ) {
                // TODO: Implement query payload serializer
                writer.rust("unimplemented!()")
            }
        }
    }

    override fun operationSerializer(operationShape: OperationShape): RuntimeType? {
        val fnName = symbolProvider.serializeFunctionName(operationShape)
        val inputShape = operationShape.inputShape(model)
        return RuntimeType.forInlineFun(fnName, "operation_ser") { writer ->
            writer.rustBlockTemplate(
                "pub fn $fnName(_input: &#{target}) -> Result<#{SdkBody}, #{Error}>",
                *codegenScope, "target" to symbolProvider.toSymbol(inputShape)
            ) {
                // TODO: Implement query operation serializer
                writer.rust("unimplemented!()")
            }
        }
    }

    override fun documentSerializer(): RuntimeType {
        TODO("AwsQuery doesn't support document types")
    }
}
