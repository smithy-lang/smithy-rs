/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class UnionWithUnitTest {
    @Test
    fun `a constrained union that has a unit member should compile`() {
        val model =
            """
            ${'$'}version: "2"
            namespace com.example
            use aws.protocols#restJson1
            use smithy.framework#ValidationException
            
            @restJson1 @title("Test Service") 
            service TestService { 
                version: "0.1", 
                operations: [ 
                    TestOperation
                    TestSimpleUnionWithUnit
                ] 
            }
            
            @http(uri: "/testunit", method: "POST")
            operation TestSimpleUnionWithUnit {
                input := {
                    @required
                    request: SomeUnionWithUnit
                }
                output := {
                    result : SomeUnionWithUnit
                }
                errors: [
                    ValidationException
                ]
            }
            
            @length(min: 13)
            string StringRestricted
            
            union SomeUnionWithUnit {
                Option1: Unit
                Option2: StringRestricted
            }

            @http(uri: "/test", method: "POST")
            operation TestOperation {
                input := { payload: String }
                output := {
                    @httpPayload
                    events: TestEvent
                },
                errors: [ValidationException]
            }
            
            @streaming
            union TestEvent {
                KeepAlive: Unit,
                Response: TestResponseEvent,
            }
            
            structure TestResponseEvent { 
                data: String 
            }            
            """.asSmithyModel()

        // Ensure the generated SDK compiles.
        serverIntegrationTest(model, testCoverage = HttpTestType.Default) { _, _ -> }
    }
}
