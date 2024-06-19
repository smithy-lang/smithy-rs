/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.traits.IsTruncatedPaginatorTrait
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.util.letIf

internal class PaginatorGeneratorTest {
    private val model =
        """
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
            inner: Inner,
            isTruncated: Boolean,
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
        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("paginators_generated") {
                Attribute.AllowUnusedImports.render(this)
                rust("use ${clientCodegenContext.moduleUseName()}::operation::paginated_list::paginator::PaginatedListPaginator;")
            }
        }
    }

    @Test
    fun `isTruncated paginators compile`() {
        // Adding IsTruncated trait to the output shape
        val modifiedModel =
            ModelTransformer.create().mapShapes(model) { shape ->
                shape.letIf(shape.isStructureShape && shape.toShapeId() == ShapeId.from("test#GetFoosOutput")) {
                    (it as StructureShape).toBuilder().addTrait(IsTruncatedPaginatorTrait()).build()
                }
            }

        clientIntegrationTest(model) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("is_truncated_paginators_generated") {
                Attribute.AllowUnusedImports.render(this)
                rust("use ${clientCodegenContext.moduleUseName()}::operation::paginated_list::paginator::PaginatedListPaginator;")
            }
        }
    }
}
