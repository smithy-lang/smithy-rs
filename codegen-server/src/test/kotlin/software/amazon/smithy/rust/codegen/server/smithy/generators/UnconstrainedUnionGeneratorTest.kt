/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverRenderWithModelBuilder
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup

class UnconstrainedUnionGeneratorTest {
    @Test
    fun `it should generate unconstrained unions`() {
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
                union: Union
            }
            
            union Union {
                structure: Structure
            }
            
            structure Structure {
                @required
                requiredMember: String
            }
            """.asSmithyModel()
        val symbolProvider = serverTestSymbolProvider(model)

        val serviceShape = model.lookup<ServiceShape>("test#TestService")
        val unionShape = model.lookup<UnionShape>("test#Union")

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(RustModule.public("model")) { writer ->
            model.lookup<StructureShape>("test#Structure").serverRenderWithModelBuilder(model, symbolProvider, writer)
        }

        val unconstrainedShapeSymbolProvider = UnconstrainedShapeSymbolProvider(symbolProvider, model, serviceShape)
        project.withModule(ModelsModule) { writer ->
            UnionGenerator(model, symbolProvider, writer, unionShape, renderUnknownVariant = false).render()
        }
        project.withModule(RustModule.private("unconstrained")) { unconstrainedModuleWriter ->
            project.withModule(ModelsModule) { modelsModuleWriter ->
                val pubCrateConstrainedShapeSymbolProvider = PubCrateConstrainedShapeSymbolProvider(symbolProvider, model, serviceShape)
                val constraintViolationSymbolProvider = ConstraintViolationSymbolProvider(symbolProvider, model, serviceShape)

                UnconstrainedUnionGenerator(
                    model,
                    symbolProvider,
                    unconstrainedShapeSymbolProvider,
                    pubCrateConstrainedShapeSymbolProvider,
                    constraintViolationSymbolProvider,
                    unconstrainedModuleWriter,
                    modelsModuleWriter,
                    unionShape
                ).render()

                unconstrainedModuleWriter.unitTest(
                    name = "unconstrained_union_fail_to_constrain",
                    test = """
                        let builder = crate::model::Structure::builder();
                        let union_unconstrained = union_unconstrained::UnionUnconstrained::Structure(builder);

                        let expected_err = crate::model::union::ConstraintViolation::StructureConstraintViolation(
                            crate::model::structure::ConstraintViolation::MissingRequiredMember,
                        );

                        assert_eq!(
                            expected_err,
                            crate::model::Union::try_from(union_unconstrained).unwrap_err()
                        );
                        """
                )

                unconstrainedModuleWriter.unitTest(
                    name = "unconstrained_union_succeed_to_constrain",
                    test = """
                        let builder = crate::model::Structure::builder().required_member(String::from("david"));
                        let union_unconstrained = union_unconstrained::UnionUnconstrained::Structure(builder);

                        let expected: crate::model::Union = crate::model::Union::Structure(crate::model::Structure {
                            required_member: String::from("david"),
                        });
                        let actual: crate::model::Union = crate::model::Union::try_from(union_unconstrained).unwrap();

                        assert_eq!(expected, actual);
                        """
                )

                unconstrainedModuleWriter.unitTest(
                    name = "unconstrained_union_converts_into_constrained",
                    test = """
                        let builder = crate::model::Structure::builder();
                        let union_unconstrained = union_unconstrained::UnionUnconstrained::Structure(builder);

                        let _union: crate::constrained::MaybeConstrained<crate::model::Union> =
                            union_unconstrained.into();
                        """
                )
                project.compileAndTest()
            }
        }
    }
}
