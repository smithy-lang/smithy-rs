/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

/**
 * Schema serde generates `deserialize_members` as an inherent method on every builder. A modeled
 * member of the same name would otherwise generate a builder setter with an identical name, which
 * is a duplicate-definition compile error rather than a silent behavior change.
 *
 * `ClientReservedWords` renames such a member, following the same mechanism already used for the
 * `build`, `builder`, and `default` builder methods. This test compiles a client whose model
 * contains exactly that member, so removing the reservation fails the build.
 */
class SchemaMemberNameCollisionTest {
    private val model =
        """
        namespace smithy.rust.codegen.test.schemaheaders

        use aws.protocols#restJson1

        @restJson1
        service CollisionService {
            version: "2023-01-01",
            operations: [GetThing],
        }

        @http(uri: "/thing", method: "GET")
        operation GetThing {
            output: GetThingOutput,
        }

        structure GetThingOutput {
            // Collides with the generated builder member consumer unless renamed.
            deserialize_members: String,

            // The established precedent for the same hazard on builder methods.
            build: String,
            builder: String,
            default: String,
        }
        """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun `a member named deserialize_members does not collide with the generated builder consumer`() {
        clientIntegrationTest(model) { codegenContext, rustCrate ->
            check(SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                "this test must exercise the schema-exclusive path, otherwise no builder consumer " +
                    "is generated and the collision cannot occur"
            }
            rustCrate.testModule {
                unitTest("the_colliding_members_are_renamed_and_still_reachable") {
                    rust(
                        """
                        // If the reservation were removed, the crate would not compile at all;
                        // these accessors additionally pin the renamed names.
                        let output = crate::operation::get_thing::GetThingOutput::builder()
                            .deserialize_members_value("a")
                            .build_value("b")
                            .builder_value("c")
                            .default_value("d")
                            .build();
                        assert_eq!(Some("a"), output.deserialize_members_value());
                        assert_eq!(Some("b"), output.build_value());
                        assert_eq!(Some("c"), output.builder_value());
                        assert_eq!(Some("d"), output.default_value());
                        """,
                    )
                }
            }
        }
    }
}
