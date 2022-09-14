/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverRenderWithModelBuilder
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup

class UnconstrainedCollectionGeneratorTest {
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
        val codegenContext = serverTestCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider

        val listA = model.lookup<ListShape>("test#ListA")
        val listB = model.lookup<ListShape>("test#ListB")

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(RustModule.public("model")) { writer ->
            model.lookup<StructureShape>("test#StructureC").serverRenderWithModelBuilder(model, symbolProvider, writer)
        }

        project.withModule(RustModule.private("constrained")) { writer ->
            listOf(listA, listB).forEach {
                PubCrateConstrainedCollectionGenerator(codegenContext, writer, it).render()
            }
        }
        project.withModule(RustModule.private("unconstrained")) { unconstrainedModuleWriter ->
            project.withModule(ModelsModule) { modelsModuleWriter ->
                listOf(listA, listB).forEach {
                    UnconstrainedCollectionGenerator(codegenContext, unconstrainedModuleWriter, modelsModuleWriter, it).render()
                }

                unconstrainedModuleWriter.unitTest(
                    name = "list_a_unconstrained_fail_to_constrain_with_first_error",
                    test = """
                    let c_builder1 = crate::model::StructureC::builder().int(69);
                    let c_builder2 = crate::model::StructureC::builder().string(String::from("david"));
                    let list_b_unconstrained = list_b_unconstrained::ListBUnconstrained(vec![c_builder1, c_builder2]);
                    let list_a_unconstrained = list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                    let expected_err =
                        crate::model::list_a::ConstraintViolation(crate::model::list_b::ConstraintViolation(
                            crate::model::structure_c::ConstraintViolation::MissingString,
                        ));
                        
                    assert_eq!(
                        expected_err,
                        crate::constrained::list_a_constrained::ListAConstrained::try_from(list_a_unconstrained).unwrap_err()
                    );
                    """,
                )

                unconstrainedModuleWriter.unitTest(
                    name = "list_a_unconstrained_succeed_to_constrain",
                    test = """
                    let c_builder = crate::model::StructureC::builder().int(69).string(String::from("david"));
                    let list_b_unconstrained = list_b_unconstrained::ListBUnconstrained(vec![c_builder]);
                    let list_a_unconstrained = list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                    let expected: Vec<Vec<crate::model::StructureC>> = vec![vec![crate::model::StructureC {
                        string: String::from("david"),
                        int: 69
                    }]];
                    let actual: Vec<Vec<crate::model::StructureC>> = 
                        crate::constrained::list_a_constrained::ListAConstrained::try_from(list_a_unconstrained).unwrap().into();
                        
                    assert_eq!(expected, actual);
                    """,
                )

                unconstrainedModuleWriter.unitTest(
                    name = "list_a_unconstrained_converts_into_constrained",
                    test = """
                    let c_builder = crate::model::StructureC::builder();
                    let list_b_unconstrained = list_b_unconstrained::ListBUnconstrained(vec![c_builder]);
                    let list_a_unconstrained = list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

                    let _list_a: crate::constrained::MaybeConstrained<crate::constrained::list_a_constrained::ListAConstrained> = list_a_unconstrained.into();
                    """,
                )
                project.compileAndTest()
            }
        }
    }
}
