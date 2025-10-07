/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

internal class RestXmlTest {
    private val model =
        """
        namespace test
        use aws.protocols#restXml
        use aws.api#service


        /// A REST XML service that sends XML requests and responses.
        @service(sdkId: "Rest XML UT")
        @restXml
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
        use aws.protocols#restXml
        use aws.api#service

        @service(sdkId: "Rest XML Empty Struct")
        @restXml
        service RestXmlEmptyStruct {
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
    fun `generate a rest xml service that compiles`() {
        clientIntegrationTest(model) { _, _ -> }
    }

    @Test
    fun `union with empty struct generates warning-free code`() {
        // This test will fail with unused variable warnings if the fix is not applied
        // clientIntegrationTest enforces -D warnings via codegenIntegrationTest
        clientIntegrationTest(modelWithEmptyStruct) { _, _ -> }
    }
}
