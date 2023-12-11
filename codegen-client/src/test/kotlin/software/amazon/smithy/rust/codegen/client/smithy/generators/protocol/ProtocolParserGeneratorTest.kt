/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class ProtocolParserGeneratorTest {
    private val model = """
        ${'$'}version: "2.0"
        namespace test
        
        use aws.protocols#restJson1
        
        @restJson1
        service TestService {
            version: "2019-12-16",
            operations: [SomeOperation]
            errors: [SomeTopLevelError]
        }
        
        @http(uri: "/SomeOperation", method: "POST")
        operation SomeOperation {
            input: SomeOperationInputOutput,
            output: SomeOperationInputOutput,
            errors: [SomeOperationError]
        }

        structure SomeOperationInputOutput {
            payload: String,
            a: String,
            b: Integer
        }
        
        @error("server")
        structure SomeTopLevelError {
            @required
            requestId: String

            @required
            message: String

            code: String = "400"

            context: String
        }
        
        @error("client")
        structure SomeOperationError {
            @required
            requestId: String

            @required
            message: String

            code: String = "400"

            context: String
        }
    """
        .asSmithyModel()

    @Test
    fun `generate an complex error structure that compiles`() {
        clientIntegrationTest(model) { _, _ -> }
    }
}
