package software.amazon.smithy.rust.codegen.server.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.testutil.AdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.Protocol
import software.amazon.smithy.rust.codegen.server.smithy.loadSmithyConstraintsModel
import software.amazon.smithy.rust.codegen.server.smithy.removeOperations
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class CborConstraintsIntegrationTest {
    @Test
    fun `ensure CBOR implementation works for all constraint types`() {
        val (serviceShape, constraintModel) = loadSmithyConstraintsModel(Protocol.Rpcv2Cbor)
        // Streaming operations are not supported by `Rpcv2Cbor` implementation.
        // https://github.com/smithy-lang/smithy-rs/issues/3573
        val nonSupportedOperations =
            listOf("EventStreamsOperation", "StreamingBlobOperation")
                .map { ShapeId.from("${serviceShape.namespace}#$it") }
        val model = constraintModel.removeOperations(serviceShape, nonSupportedOperations)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShape.toString(),
                additionalSettings = AdditionalSettings.GenerateCodegenComments().toObjectNode(),
            ),
        ) { codegenContext, rustCrate ->
            fun RustWriter.testTypeExistsInBuilderModule(typeName: String) {
                unitTest(
                    "builder_module_has_${typeName.toSnakeCase()}",
                    """
                    
                    """,
                )
            }
        }
    }
}
