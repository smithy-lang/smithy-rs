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
     * Test model with a union containing an empty struct member.
     * Regression test for https://github.com/smithy-lang/smithy-rs/issues/4308
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

        union TestUnion {
            emptyStructMember: EmptyStruct,
            dataMember: String
        }

        structure EmptyStruct {
            // NO MEMBERS
        }

        @input
        structure TestOperationInput {
            union: TestUnion
        }

        @http(uri: "/test", method: "POST")
        operation TestOperation {
            input: TestOperationInput,
        }
        """.asSmithyModel()

    /**
     * Shape IDs for the union with empty struct model.
     */
    object UnionWithEmptyStructShapeIds {
        const val TEST_UNION = "test#TestUnion" 
        const val EMPTY_STRUCT = "test#EmptyStruct"
        const val TEST_INPUT = "test#TestOperationInput"
        const val TEST_OPERATION = "test#TestOperation"
    }
}
