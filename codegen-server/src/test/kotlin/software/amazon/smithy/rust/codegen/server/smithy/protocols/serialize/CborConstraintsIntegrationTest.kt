package software.amazon.smithy.rust.codegen.server.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.server.smithy.ModelProtocol
import software.amazon.smithy.rust.codegen.server.smithy.loadSmithyConstraintsModelForProtocol
import software.amazon.smithy.rust.codegen.server.smithy.removeOperations
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class CborConstraintsIntegrationTest {
    @Test
    fun `ensure CBOR implementation works for all constraint types`() {
        val (serviceShape, constraintModel) = loadSmithyConstraintsModelForProtocol(ModelProtocol.Rpcv2Cbor)
        // Event streaming operations are not supported by `Rpcv2Cbor` implementation.
        // https://github.com/smithy-lang/smithy-rs/issues/3573
        val nonSupportedOperations =
            listOf("StreamingBlobOperation")
                .map { ShapeId.from("${serviceShape.namespace}#$it") }
        val model =
            constraintModel
                .removeOperations(serviceShape, nonSupportedOperations)
        // The test should compile; no further testing is required.
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShape.toString(),
                additionalSettings = ServerAdditionalSettings.builder().generateCodegenComments().toObjectNode(),
            ),
        ) { _, _ ->
        }
    }
}
