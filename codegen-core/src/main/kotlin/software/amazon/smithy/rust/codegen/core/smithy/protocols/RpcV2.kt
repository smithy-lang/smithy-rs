package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.StructuredDataParserGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.StructuredDataSerializerGenerator

open class RpcV2(val codegenContext: CodegenContext) : Protocol {
    override val httpBindingResolver: HttpBindingResolver
        get() = TODO("Not yet implemented")

    override val defaultTimestampFormat: TimestampFormatTrait.Format =
        TimestampFormatTrait.Format.DATE_TIME

    override fun structuredDataParser(operationShape: OperationShape): StructuredDataParserGenerator {
        TODO("Not yet implemented")
    }

    override fun structuredDataSerializer(operationShape: OperationShape): StructuredDataSerializerGenerator {
        TODO("Not yet implemented")
    }

    override fun parseHttpErrorMetadata(operationShape: OperationShape): RuntimeType {
        TODO("Not yet implemented")
    }

    override fun parseEventStreamErrorMetadata(operationShape: OperationShape): RuntimeType {
        TODO("Not yet implemented")
    }
}
