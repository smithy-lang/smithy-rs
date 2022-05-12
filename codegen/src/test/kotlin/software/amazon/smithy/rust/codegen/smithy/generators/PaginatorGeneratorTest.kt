/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.RustCodegenPlugin
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.util.runCommand

internal class PaginatorGeneratorTest {
    private val model = """
        namespace test
        use aws.protocols#awsJson1_1

        @awsJson1_1
        service TestService {
            operations: [PaginatedList, PaginatedMap]
        }

        @readonly
        @paginated(inputToken: "nextToken", outputToken: "inner.token",
                   pageSize: "maxResults", items: "inner.items")
        operation PaginatedList {
            input: GetFoosInput,
            output: GetFoosOutput
        }

        @readonly
        @paginated(inputToken: "nextToken", outputToken: "inner.token",
                   pageSize: "maxResults", items: "inner.mapItems")
        operation PaginatedMap {
            input: GetFoosInput,
            output: GetFoosOutput
        }

        structure GetFoosInput {
            maxResults: Integer,
            nextToken: String
        }

        structure Inner {
            token: String,

            @required
            items: StringList,

            @required
            mapItems: StringMap
        }

        structure GetFoosOutput {
            inner: Inner
        }

        list StringList {
            member: String
        }

        map StringMap {
            key: String,
            value: Integer
        }
    """.asSmithyModel()

    @Test
    fun `generate paginators that compile`() {
        val (ctx, testDir) = generatePluginContext(model)
        RustCodegenPlugin().execute(ctx)
        "cargo test".runCommand(testDir)
    }
}
