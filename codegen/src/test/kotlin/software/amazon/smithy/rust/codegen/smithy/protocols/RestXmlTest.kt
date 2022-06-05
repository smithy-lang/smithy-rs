/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.RustCodegenPlugin
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.util.runCommand

internal class RestXmlTest {

    private val model = """
        namespace test
        use aws.protocols#restXml
        use aws.api#service


        /// A REST XML service that sends XML requests and responses.
        @service(sdkId: "Rest XML UT")
        @restXml
        service RestXmlExtras {
            version: "2019-12-16",
            operations: [Op]
        }


        @http(uri: "/top", method: "POST")
        operation Op {
            input: Top,
            output: Top
        }
        union Choice {
            @xmlFlattened
            @xmlName("Hi")
            flatMap: MyMap,

            deepMap: MyMap,

            @xmlFlattened
            flatList: SomeList,

            deepList: SomeList,

            s: String,

            enum: FooEnum,

            date: Timestamp,

            number: Double,

            top: Top,

            blob: Blob
        }

        @enum([{name: "FOO", value: "FOO"}])
        string FooEnum

        map MyMap {
            @xmlName("Name")
            key: String,

            @xmlName("Setting")
            value: Choice,
        }

        list SomeList {
            member: Choice
        }

        structure Top {
            choice: Choice,

            @xmlAttribute
            extra: Long,

            @xmlName("prefix:local")
            renamedWithPrefix: String
        }

    """.asSmithyModel()

    @Test
    fun `generate a rest xml service that compiles`() {
        val (pluginContext, testDir) = generatePluginContext(model)
        RustCodegenPlugin().execute(pluginContext)
        "cargo check".runCommand(testDir)
    }
}
