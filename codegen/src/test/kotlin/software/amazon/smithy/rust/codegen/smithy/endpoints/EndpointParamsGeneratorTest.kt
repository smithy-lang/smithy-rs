/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.endpoints

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.MethodSource
import software.amazon.smithy.rulesengine.testutil.TestDiscovery
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.unitTest
import java.util.stream.Stream

internal class EndpointParamsGeneratorTest {
    companion object {
        @JvmStatic
        fun testSuites(): Stream<TestDiscovery.RulesTestSuite> = TestDiscovery().testSuites()
    }

    @ParameterizedTest()
    @MethodSource("testSuites")
    fun `generate endpoint params for provided test suites`(testSuite: TestDiscovery.RulesTestSuite) {
        val project = TestWorkspace.testProject()
        project.lib { writer ->
            writer.unitTest("params_work") {
                rustTemplate(
                    """
                    // this might fail if there are required fields
                    let _ = #{Params}::builder().build();
                    """,
                    "Params" to EndpointParamsGenerator(testSuite.ruleset()).paramsStruct(),
                )
            }
        }
        project.compileAndTest()
    }
}
