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

    // Regression: when a @paginated operation's top-level outputToken targets a @required member,
    // the borrowing lens accessor in src/lens.rs previously moved the owned value instead of
    // borrowing it, producing Option<String> where Option<&String> is expected.
    @Test
    fun `paginators with required top-level outputToken compile`() {
        val requiredTokenModel =
            """
            namespace test
            use aws.protocols#awsJson1_1

            @awsJson1_1
            service TestService {
                operations: [ListThings]
            }

            @readonly
            @optionalAuth
            @paginated(inputToken: "nextToken", outputToken: "nextToken",
                       pageSize: "maxResults", items: "thingSummaries")
            operation ListThings {
                input: ListThingsInput,
                output: ListThingsOutput
            }

            structure ListThingsInput {
                maxResults: Integer,
                nextToken: String
            }

            structure ListThingsOutput {
                @required
                nextToken: String,

                @required
                thingSummaries: StringList
            }

            list StringList {
                member: String
            }
            // disableValidation: Smithy's paginated-trait lint flags a @required outputToken; we intentionally model it that way to exercise the codegen path.
            """.asSmithyModel(disableValidation = true)

        clientIntegrationTest(requiredTokenModel) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("paginators_required_top_level_token") {
                Attribute.AllowUnusedImports.render(this)
                rust("use ${clientCodegenContext.moduleUseName()}::operation::list_things::paginator::ListThingsPaginator;")
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
