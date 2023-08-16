/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.testClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.validateConfigCustomizations
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

    @Test
    fun `client params generate a valid customization`() {
        val project = TestWorkspace.testProject()
        val context = testClientCodegenContext(model)
        project.unitTest {
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
        }
        // unset fields
        project.unitTest {
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
        }
        validateConfigCustomizations(context, ClientContextConfigCustomization(context), project)
    }
}
