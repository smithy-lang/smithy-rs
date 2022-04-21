/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MapShape
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

class UnconstrainedMapGeneratorTest {
    @Test
    fun `it should generate unconstrained maps`() {
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
                map: MapA
            }
            
            map MapA {
                key: String,
                value: MapB
            }
            
            map MapB {
                key: String,
                value: StructureC
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
        val mapA = model.lookup<MapShape>("test#MapA")
        val mapB = model.lookup<MapShape>("test#MapB")

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(RustModule.public("model")) { writer ->
            model.lookup<StructureShape>("test#StructureC").serverRenderWithModelBuilder(model, symbolProvider, writer)
        }

        project.withModule(RustModule.private("unconstrained")) { writer ->
            val unconstrainedShapeSymbolProvider = UnconstrainedShapeSymbolProvider(symbolProvider, model, serviceShape)
            val constraintViolationSymbolProvider = ConstraintViolationSymbolProvider(symbolProvider, model, serviceShape)
            listOf(mapA, mapB).forEach {
                UnconstrainedMapGenerator(
                    model,
                    symbolProvider,
                    unconstrainedShapeSymbolProvider,
                    constraintViolationSymbolProvider,
                    writer,
                    it
                ).render()
            }

            // TODO This test is flaky because it depends on the order in which the `HashMap` is visited.
//            writer.unitTest(
//                name = "map_a_unconstrained_fail_to_constrain_with_first_error",
//                test = """
//                    let c_builder1 = crate::model::StructureC::builder().int(69);
//                    let c_builder2 = crate::model::StructureC::builder().string(String::from("david"));
//                    let map_b_unconstrained = map_b_unconstrained::MapBUnconstrained(
//                        std::collections::HashMap::from([
//                            (String::from("KeyB1"), c_builder1),
//                            (String::from("KeyB2"), c_builder2),
//                        ])
//                    );
//                    let map_a_unconstrained = map_a_unconstrained::MapAUnconstrained(
//                        std::collections::HashMap::from([
//                            (String::from("KeyA"), map_b_unconstrained),
//                        ])
//                    );
//
//                    let expected_err =
//                        map_a_unconstrained::ConstraintViolation::Value(map_b_unconstrained::ConstraintViolation::Value(
//                            crate::model::structure_c::ConstraintViolation::MissingString,
//                        ));
//
//                    use std::convert::TryFrom;
//                    assert_eq!(
//                        expected_err,
//                        std::collections::HashMap::<String, std::collections::HashMap<String, crate::model::StructureC>>::try_from(map_a_unconstrained).unwrap_err()
//                    );
//                """
//            )

            writer.unitTest(
                name = "map_a_unconstrained_succeed_to_constrain",
                test = """
                    let c_builder = crate::model::StructureC::builder().int(69).string(String::from("david"));
                    let map_b_unconstrained = map_b_unconstrained::MapBUnconstrained(
                        std::collections::HashMap::from([
                            (String::from("KeyB"), c_builder),
                        ])
                    );
                    let map_a_unconstrained = map_a_unconstrained::MapAUnconstrained(
                        std::collections::HashMap::from([
                            (String::from("KeyA"), map_b_unconstrained),
                        ])
                    );

                    let expected = std::collections::HashMap::from([
                        (String::from("KeyA"), std::collections::HashMap::from([
                            (String::from("KeyB"), crate::model::StructureC {
                                int: 69,
                                string: String::from("david")
                            }),
                        ]))
                    ]);

                    use std::convert::TryFrom;
                    assert_eq!(
                        expected,
                        std::collections::HashMap::<String, std::collections::HashMap<String, crate::model::StructureC>>::try_from(map_a_unconstrained).unwrap()
                    );
                """
            )

            writer.unitTest(
                name = "map_a_unconstrained_converts_into_constrained",
                test = """
                    let c_builder = crate::model::StructureC::builder();
                    let map_b_unconstrained = map_b_unconstrained::MapBUnconstrained(
                        std::collections::HashMap::from([
                            (String::from("KeyB"), c_builder),
                        ])
                    );
                    let map_a_unconstrained = map_a_unconstrained::MapAUnconstrained(
                        std::collections::HashMap::from([
                            (String::from("KeyA"), map_b_unconstrained),
                        ])
                    );

                    let _map_a: crate::constrained::MaybeConstrained<std::collections::HashMap<String, std::collections::HashMap<String, crate::model::StructureC>>> = map_a_unconstrained.into();
                """
            )

            project.compileAndTest()
        }
    }
}
