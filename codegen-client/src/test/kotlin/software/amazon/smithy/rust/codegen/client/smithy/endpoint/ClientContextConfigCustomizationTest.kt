/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class ClientContextConfigCustomizationTest {
    val model = """
        namespace test
        use smithy.rules#clientContextParams

        @clientContextParams(aStringParam: {
            documentation: "string docs",
            type: "string"
        },
        aBoolParam: {
            documentation: "bool docs",
            type: "boolean"
        })
        service TestService { operations: [] }
    """.asSmithyModel()

    @Test
    fun `client params generate a valid customization`() {
        val project = TestWorkspace.testProject()
        project.unitTest {
            rust(
                """
                let conf = crate::Config::builder().a_string_param("hello!").a_bool_param(true).build();
                assert_eq!(conf.a_string_param.unwrap(), "hello!");
                assert_eq!(conf.a_bool_param, Some(true));
                """,
            )
        }
        // unset fields
        project.unitTest {
            rust(
                """
                let conf = crate::Config::builder().a_string_param("hello!").build();
                assert_eq!(conf.a_string_param.unwrap(), "hello!");
                assert_eq!(conf.a_bool_param, None);
                """,
            )
        }
        validateConfigCustomizations(ClientContextConfigCustomization(testClientCodegenContext(model)), project)
    }
}
