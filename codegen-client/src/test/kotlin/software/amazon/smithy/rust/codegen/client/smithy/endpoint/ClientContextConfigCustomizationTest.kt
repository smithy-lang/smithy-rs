/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class ClientContextConfigCustomizationTest {
    val model = """
        namespace test
        use smithy.rules#clientContextParams
        use aws.protocols#awsJson1_0

        @clientContextParams(aStringParam: {
            documentation: "string docs",
            type: "string"
        },
        aBoolParam: {
            documentation: "bool docs",
            type: "boolean"
        })
        @awsJson1_0
        service TestService { operations: [] }
    """.asSmithyModel()

    @Test
    fun `client params generate a valid customization`() {
        clientIntegrationTest(model) { _, crate ->
            crate.unitTest {
                rustTemplate(
                    """
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
                )
            }

            crate.unitTest("unset_fields") {
                rustTemplate(
                    """
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
                )
            }
        }
    }
}
