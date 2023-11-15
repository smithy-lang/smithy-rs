/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

object BasicTestModels {
    val AwsJson10TestModel = """
        namespace com.example
        use aws.protocols#awsJson1_0
        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }
        @optionalAuth
        operation SayHello { input: TestInput }
        structure TestInput {
           foo: String,
        }
    """.asSmithyModel()
}
