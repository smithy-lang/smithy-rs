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

    private val modelWithEmptyStruct =
        """
        namespace test
        use aws.protocols#restJson1
        use aws.api#service

        @service(sdkId: "Rest Json Empty Struct")
        @restJson1
        service RestJsonEmptyStruct {
            version: "2019-12-16",
            operations: [TestOp]
        }

        @http(uri: "/test", method: "POST")
        operation TestOp {
            input: TestInput,
            output: TestOutput
        }

        structure TestInput {
            testUnion: TestUnion
        }

        structure TestOutput {
            testUnion: TestUnion
        }

        union TestUnion {
            // Empty struct - RestJson ALWAYS uses inner variable, no warning
            emptyStruct: EmptyStruct,

            // Normal struct - RestJson uses inner variable
            normalStruct: NormalStruct
        }

        structure EmptyStruct {}

        structure NormalStruct {
            value: String
        }
        """.asSmithyModel()

    @Test
    fun `generate a rest json service that compiles`() {
        clientIntegrationTest(model) { _, _ -> }
    }

    @Test
    fun `union with empty struct always uses inner variable`() {
        // This test documents that RestJson protocol is immune to unused variable issues.
        // Unlike RestXml/AwsQuery, RestJson serializers always reference the inner variable
        // even for empty structs, so no underscore prefix is needed.
        // This test passes without any code changes, proving RestJson immunity.
        clientIntegrationTest(modelWithEmptyStruct) { _, _ -> }
    }
}
