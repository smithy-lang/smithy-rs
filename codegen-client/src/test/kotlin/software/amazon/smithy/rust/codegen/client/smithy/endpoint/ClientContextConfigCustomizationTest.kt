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
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
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
        val context = testClientCodegenContext(model).withSmithyRuntimeMode(smithyRuntimeMode)
        project.unitTest {
            if (smithyRuntimeMode.generateOrchestrator) {
                rustTemplate(
                    """
                    use #{RuntimePlugin};
                    let conf = crate::Config::builder().a_string_param("hello!").a_bool_param(true).build();
                    assert_eq!(
                        conf.config
                            .load::<crate::config::AStringParam>()
                            .map(|u| u.0.clone())
                            .unwrap(),
                        "hello!"
                    );
                    assert_eq!(
                        conf.config
                            .load::<crate::config::ABoolParam>()
                            .map(|u| u.0),
                        Some(true)
                    );
                    """,
                    "RuntimePlugin" to RuntimeType.runtimePlugin(context.runtimeConfig),
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
            if (smithyRuntimeMode.generateOrchestrator) {
                rustTemplate(
                    """
                    use #{RuntimePlugin};
                    let conf = crate::Config::builder().a_string_param("hello!").build();
                    assert_eq!(
                        conf.config
                            .load::<crate::config::AStringParam>()
                            .map(|u| u.0.clone())
                            .unwrap(),
                        "hello!"
                    );
                    assert_eq!(
                        conf.config
                            .load::<crate::config::ABoolParam>()
                            .map(|u| u.0),
                        None,
                    );
                    """,
                    "RuntimePlugin" to RuntimeType.runtimePlugin(context.runtimeConfig),
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
        validateConfigCustomizations(context, ClientContextConfigCustomization(context), project)
    }
}
