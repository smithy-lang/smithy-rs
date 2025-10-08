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
            version: "2019-12-16",
            operations: [SomeOperation]
        }

        operation SomeOperation {
            input: SomeOperationInputOutput,
            output: SomeOperationInputOutput,
        }

        structure SomeOperationInputOutput {
            payload: String,
            a: String,
            b: Integer
        }
        """.asSmithyModel()

    private val modelWithEmptyStruct =
        """
        namespace test
        use aws.protocols#awsQuery

        @awsQuery
        @xmlNamespace(uri: "https://example.com/")
        service TestService {
            version: "2019-12-16",
            operations: [TestOp]
        }

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
            // Empty struct - should generate _inner to avoid unused variable warning
            emptyStruct: EmptyStruct,

            // Normal struct - should generate inner (without underscore)
            normalStruct: NormalStruct
        }

        structure EmptyStruct {}

        structure NormalStruct {
            value: String
        }
        """.asSmithyModel()

    @Test
    fun `generate an aws query service that compiles`() {
        clientIntegrationTest(model) { _, _ -> }
    }

    @Test
    fun `union with empty struct generates warning-free code`() {
        // This test will fail with unused variable warnings if the fix is not applied
        // clientIntegrationTest enforces -D warnings via codegenIntegrationTest
        clientIntegrationTest(modelWithEmptyStruct) { _, _ -> }
    }
}
