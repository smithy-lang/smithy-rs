/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class ServerBuilderConstraintViolationsTest {

    // This test exists not to regress on [this](https://github.com/awslabs/smithy-rs/issues/2343) issue.
    // We generated constraint violation variants, pointing to a structure (StructWithInnerDefault below),
    // but the structure was not constrained, because the structure's member have a default value
    // and default values are validated at generation time from the model.
    @Test
    fun `it should not generate constraint violations for members with a default value`() {
        val model = """
            namespace test

            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service SimpleService {
                operations: [Operation]
            }

            @http(uri: "/operation", method: "POST")
            operation Operation {
                input: OperationInput
                errors: [ValidationException]
            }

            structure OperationInput {
                @required
                requiredStructureWithInnerDefault: StructWithInnerDefault
            }

            structure StructWithInnerDefault {
                @default(false)
                inner: PrimitiveBoolean
            }
        """.asSmithyModel(smithyVersion = "2")
        serverIntegrationTest(model)
    }
}
