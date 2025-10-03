/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class AwsQueryTest {
    private val model =
        """
        namespace test
        use aws.protocols#awsQuery

        @awsQuery
        @xmlNamespace(uri: "https://example.com/")
        service TestService {
            version: "1.0",
            operations: [TestOperation]
        }

        union ObjectEncryptionFilter {
            sses3: SSES3Filter,
        }

        structure SSES3Filter {
            // Empty structure - no members
        }

        @input
        structure TestInput {
            filter: ObjectEncryptionFilter
        }

        operation TestOperation {
            input: TestInput,
        }
        """.asSmithyModel()

    @Test
    fun `generate an aws query service that compiles`() {
        clientIntegrationTest(model) { _, _ -> }
    }
}
