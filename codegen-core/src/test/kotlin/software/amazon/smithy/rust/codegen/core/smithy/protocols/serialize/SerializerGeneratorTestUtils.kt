/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

/**
 * Shared test utilities for serializer generator tests.
 * Contains common test models and utilities to avoid duplication across protocol tests.
 */
object SerializerGeneratorTestUtils {
    /**
     * Test model based on S3Control ObjectEncryptionFilter pattern.
     * Regression test for https://github.com/smithy-lang/smithy-rs/issues/4308
     * 
     * This model creates a union with an empty struct that should generate unused `inner` variables.
     */
    val unionWithEmptyStructModel =
        """
        namespace test
        use aws.protocols#restJson1

        @restJson1
        service TestService {
            version: "1.0",
            operations: [TestOperation]
        }

        union ObjectEncryptionFilter {
            sses3: SSES3Filter,
            data: String
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

    /**
     * Shape IDs for the union with empty struct model.
     */
    object UnionWithEmptyStructShapeIds {
        const val TEST_UNION = "test#ObjectEncryptionFilter" 
        const val EMPTY_STRUCT = "test#SSES3Filter"
        const val TEST_INPUT = "test#TestInput"
        const val TEST_OPERATION = "test#TestOperation"
    }
}
