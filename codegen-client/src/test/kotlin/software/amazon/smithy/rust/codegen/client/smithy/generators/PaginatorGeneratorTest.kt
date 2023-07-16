/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.TestCodegenSettings
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

internal class PaginatorGeneratorTest {
    private val model = """
        namespace test
        use aws.protocols#awsJson1_1

        @awsJson1_1
        service TestService {
            operations: [PaginatedList, PaginatedMap]
        }

        @readonly
        @optionalAuth
        @paginated(inputToken: "nextToken", outputToken: "inner.token",
                   pageSize: "maxResults", items: "inner.items")
        operation PaginatedList {
            input: GetFoosInput,
            output: GetFoosOutput
        }

        @readonly
        @optionalAuth
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

    // TODO(enableNewSmithyRuntimeCleanup): Remove this middleware test when launching
    @Test
    fun `generate paginators that compile with middleware`() {
        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("paginators_generated") {
                Attribute.AllowUnusedImports.render(this)
                rust("use ${clientCodegenContext.moduleUseName()}::operation::paginated_list::paginator::PaginatedListPaginator;")
            }
        }
    }

    @Test
    fun `generate paginators that compile`() {
        clientIntegrationTest(
            model,
            params = IntegrationTestParams(additionalSettings = TestCodegenSettings.orchestratorMode()),
        ) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("paginators_generated") {
                Attribute.AllowUnusedImports.render(this)
                rust("use ${clientCodegenContext.moduleUseName()}::operation::paginated_list::paginator::PaginatedListPaginator;")
            }
        }
    }
}
