/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.client.smithy.SmithyRuntimeMode
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
import software.amazon.smithy.rust.codegen.client.testutil.withSmithyRuntimeMode
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

    @ParameterizedTest
    @ValueSource(strings = ["middleware", "orchestrator"])
    fun `client params generate a valid customization`(smithyRuntimeModeStr: String) {
        val project = TestWorkspace.testProject()
        val smithyRuntimeMode = SmithyRuntimeMode.fromString(smithyRuntimeModeStr)
        project.unitTest {
            if (smithyRuntimeMode.defaultToOrchestrator) {
                rust(
                    """
                    let conf = crate::Config::builder().a_string_param("hello!").a_bool_param(true).build();
                    assert_eq!(conf.a_string_param().unwrap(), "hello!");
                    assert_eq!(conf.a_bool_param(), Some(true));
                    """,
                )
            } else {
                rust(
                    """
                    let conf = crate::Config::builder().a_string_param("hello!").a_bool_param(true).build();
                    assert_eq!(conf.a_string_param.unwrap(), "hello!");
                    assert_eq!(conf.a_bool_param, Some(true));
                    """,
                )
            }
        }
        // unset fields
        project.unitTest {
            if (smithyRuntimeMode.defaultToOrchestrator) {
                rust(
                    """
                    let conf = crate::Config::builder().a_string_param("hello!").build();
                    assert_eq!(conf.a_string_param().unwrap(), "hello!");
                    assert_eq!(conf.a_bool_param(), None);
                    """,
                )
            } else {
                rust(
                    """
                    let conf = crate::Config::builder().a_string_param("hello!").build();
                    assert_eq!(conf.a_string_param.unwrap(), "hello!");
                    assert_eq!(conf.a_bool_param, None);
                    """,
                )
            }
        }
        val context = testClientCodegenContext(model).withSmithyRuntimeMode(smithyRuntimeMode)
        validateConfigCustomizations(context, ClientContextConfigCustomization(context), project)
    }
}
