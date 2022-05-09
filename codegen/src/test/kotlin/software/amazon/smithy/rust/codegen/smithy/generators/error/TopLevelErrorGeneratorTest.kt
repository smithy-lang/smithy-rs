/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.error

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.RustCodegenPlugin
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.util.runCommand
import kotlin.io.path.ExperimentalPathApi
import kotlin.io.path.createDirectory
import kotlin.io.path.writeText

internal class TopLevelErrorGeneratorTest {
    @ExperimentalPathApi
    @Test
    fun `top level errors are send + sync`() {
        val model = """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1
            service HelloService {
                operations: [SayHello],
                version: "1"
            }

            @http(uri: "/", method: "POST")
            operation SayHello {
                errors: [SorryBusy, CanYouRepeatThat]

            }

            @error("server")
            structure SorryBusy { }

            @error("client")
            structure CanYouRepeatThat { }
        """.asSmithyModel()

        val (pluginContext, testDir) = generatePluginContext(model)
        val moduleName = pluginContext.settings.expectStringMember("module").value.replace('-', '_')
        RustCodegenPlugin().execute(pluginContext)
        testDir.resolve("tests").createDirectory()
        testDir.resolve("tests/validate_errors.rs").writeText(
            """
            fn check_send_sync<T: Send + Sync>() {}
            #[test]
            fn tl_errors_are_send_sync() {
                check_send_sync::<$moduleName::Error>()
            }
            """
        )
        "cargo test".runCommand(testDir)
    }
}
