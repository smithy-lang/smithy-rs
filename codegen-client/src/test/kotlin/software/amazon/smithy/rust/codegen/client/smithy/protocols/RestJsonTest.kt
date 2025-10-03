/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

internal class RestJsonTest {
    val model =
        """
        namespace test
        use aws.protocols#restJson1
        use aws.api#service

        @restJson1
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

        @http(uri: "/test", method: "POST")
        operation TestOperation {
            input: TestInput,
        }
        """.asSmithyModel()

    @Test
    fun `generate a rest json service that compiles`() {
        clientIntegrationTest(model) { _, _ -> }
    }
}
