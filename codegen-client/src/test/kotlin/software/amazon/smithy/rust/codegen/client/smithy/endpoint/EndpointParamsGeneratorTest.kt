/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

internal class EndpointParamsGeneratorTest {
    /*
    companion object {
        @JvmStatic
        fun testSuites(): Stream<TestDiscovery.RulesTestSuite> = TestDiscovery().testSuites()
    }

    @ParameterizedTest()
    @MethodSource("testSuites")
    fun `generate endpoint params for provided test suites`(testSuite: TestDiscovery.RulesTestSuite) {
        val project = TestWorkspace.testProject()
        val context = testClientCodegenContext()
        project.lib {
            unitTest("params_work") {
                rustTemplate(
                    """
                    // this might fail if there are required fields
                    let _ = #{Params}::builder().build();
                    """,
                    "Params" to EndpointParamsGenerator(context, testSuite.ruleSet().parameters).paramsStruct(),
                )
            }
        }
        project.compileAndTest()
    }*/
}
