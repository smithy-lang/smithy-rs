package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

class ServerErrorReachableShapesDisplayTest {
    @Test
    fun `composite error shapes are compilable`() {
        var sampleModel = File("../codegen-core/common-test-models/nested-error.smithy").readText().asSmithyModel()
        serverIntegrationTest(sampleModel) { _, _ ->
            // It should compile.
        }
    }
}
