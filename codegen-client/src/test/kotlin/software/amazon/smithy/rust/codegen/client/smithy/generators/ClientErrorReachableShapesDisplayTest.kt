package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import java.io.File

class ClientErrorReachableShapesDisplayTest {
    @Test
    fun correctMissingFields() {
        var sampleModel = File("../codegen-core/common-test-models/nested-error.smithy").readText().asSmithyModel()
        clientIntegrationTest(sampleModel) { _, _ ->
            // It should compile.
        }
    }
}
