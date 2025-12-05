/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class UnconstrainedCollectionGeneratorTest {
    @Test
    fun `it should generate unconstrained lists`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service TestService {
                operations: ["Operation"]
            }

            @http(uri: "/operation", method: "POST")
            operation Operation {
                input: OperationInputOutput
                output: OperationInputOutput
                errors: [ValidationException]
            }

            structure OperationInputOutput {
                list: ListA
            }

            list ListA {
                member: ListB
            }

            list ListB {
                member: StructureC
            }

            structure StructureC {
                @required
                int: Integer,

                @required
                string: String
            }
            """.asSmithyModel()

        serverIntegrationTest(model, testCoverage = HttpTestType.AsConfigured) { _, rustCrate ->
            rustCrate.testModule {
                unitTest("list_a_unconstrained_fail_to_constrain_with_first_error") {
                    rust(
                        """
                        let c_builder1 = crate::model::StructureC::builder().int(69);
                        let c_builder2 = crate::model::StructureC::builder().string("david".to_owned());
                        let list_b_unconstrained = crate::unconstrained::list_b_unconstrained::ListBUnconstrained(vec![c_builder1, c_builder2]);
                        let list_a_unconstrained = crate::unconstrained::list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                        let expected_err =
                            crate::model::list_a::ConstraintViolation::Member(0, crate::model::list_b::ConstraintViolation::Member(
                                0, crate::model::structure_c::ConstraintViolation::MissingString,
                            ));

                        assert_eq!(
                            expected_err,
                            crate::constrained::list_a_constrained::ListAConstrained::try_from(list_a_unconstrained).unwrap_err()
                        );
                        """,
                    )
                }

                unitTest("list_a_unconstrained_succeed_to_constrain") {
                    rust(
                        """
                        let c_builder = crate::model::StructureC::builder().int(69).string(String::from("david"));
                        let list_b_unconstrained = crate::unconstrained::list_b_unconstrained::ListBUnconstrained(vec![c_builder]);
                        let list_a_unconstrained = crate::unconstrained::list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                        let expected: Vec<Vec<crate::model::StructureC>> = vec![vec![crate::model::StructureC {
                            string: "david".to_owned(),
                            int: 69
                        }]];
                        let actual: Vec<Vec<crate::model::StructureC>> =
                            crate::constrained::list_a_constrained::ListAConstrained::try_from(list_a_unconstrained).unwrap().into();

                        assert_eq!(expected, actual);
                        """,
                    )
                }

                unitTest("list_a_unconstrained_converts_into_constrained") {
                    rust(
                        """
                        let c_builder = crate::model::StructureC::builder();
                        let list_b_unconstrained = crate::unconstrained::list_b_unconstrained::ListBUnconstrained(vec![c_builder]);
                        let list_a_unconstrained = crate::unconstrained::list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                        let _list_a: crate::constrained::MaybeConstrained<crate::constrained::list_a_constrained::ListAConstrained> = list_a_unconstrained.into();
                        """,
                    )
                }
            }
        }
    }
}
