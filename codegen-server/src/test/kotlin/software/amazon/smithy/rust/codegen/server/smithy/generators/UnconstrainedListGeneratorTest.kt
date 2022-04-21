/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverRenderWithModelBuilder
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup

class UnconstrainedListGeneratorTest {
    @Test
    fun `it should generate unconstrained lists`() {
        val model =
            """
            namespace test

            service TestService {
                version: "123",
                operations: [TestOperation]
            }
            
            operation TestOperation {
                input: TestInputOutput,
                output: TestInputOutput,
            }
            
            structure TestInputOutput {
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
        val symbolProvider = serverTestSymbolProvider(model)

        val serviceShape = model.lookup<ServiceShape>("test#TestService")
        val listA = model.lookup<ListShape>("test#ListA")
        val listB = model.lookup<ListShape>("test#ListB")

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(RustModule.public("model")) { writer ->
            model.lookup<StructureShape>("test#StructureC").serverRenderWithModelBuilder(model, symbolProvider, writer)
        }

        project.withModule(RustModule.private("unconstrained")) { writer ->
            val unconstrainedShapeSymbolProvider = UnconstrainedShapeSymbolProvider(symbolProvider, model, serviceShape)
            val constraintViolationSymbolProvider = ConstraintViolationSymbolProvider(symbolProvider, model, serviceShape)
            listOf(listA, listB).forEach {
                UnconstrainedListGenerator(
                    model,
                    symbolProvider,
                    unconstrainedShapeSymbolProvider,
                    constraintViolationSymbolProvider,
                    writer,
                    it
                ).render()
            }

            writer.unitTest(
                name = "list_a_unconstrained_fail_to_constrain_with_first_error",
                test = """
                    let c_builder1 = crate::model::StructureC::builder().int(69);
                    let c_builder2 = crate::model::StructureC::builder().string(String::from("david"));
                    let list_b_unconstrained = list_b_unconstrained::ListBUnconstrained(vec![c_builder1, c_builder2]);
                    let list_a_unconstrained = list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                    let expected_err =
                        list_a_unconstrained::ConstraintViolation(list_b_unconstrained::ConstraintViolation(
                            crate::model::structure_c::ConstraintViolation::MissingString,
                        ));

                    use std::convert::TryFrom;
                    assert_eq!(
                        expected_err,
                        Vec::<Vec<crate::model::StructureC>>::try_from(list_a_unconstrained).unwrap_err()
                    );
                """
            )

            writer.unitTest(
                name = "list_a_unconstrained_succeed_to_constrain",
                test = """
                    let c_builder = crate::model::StructureC::builder().int(69).string(String::from("david"));
                    let list_b_unconstrained = list_b_unconstrained::ListBUnconstrained(vec![c_builder]);
                    let list_a_unconstrained = list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                    let expected = vec![vec![crate::model::StructureC {
                        string: String::from("david"),
                        int: 69
                    }]];

                    use std::convert::TryFrom;
                    assert_eq!(
                        expected,
                        Vec::<Vec<crate::model::StructureC>>::try_from(list_a_unconstrained).unwrap()
                    );
                """
            )

            writer.unitTest(
                name = "list_a_unconstrained_converts_into_constrained",
                test = """
                    let c_builder = crate::model::StructureC::builder();
                    let list_b_unconstrained = list_b_unconstrained::ListBUnconstrained(vec![c_builder]);
                    let list_a_unconstrained = list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                    let _list_a: crate::constrained::MaybeConstrained<Vec<Vec<crate::model::StructureC>>> = list_a_unconstrained.into();
                """
            )

            project.compileAndTest()
        }
    }
}
