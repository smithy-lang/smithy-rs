/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.createInlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.renderInlineMemoryModules
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverRenderWithModelBuilder
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class UnconstrainedUnionGeneratorTest {
    @Test
    fun `it should generate unconstrained unions`() {
        val model =
            """
            namespace test

            union Union {
                structure: Structure
            }

            structure Structure {
                @required
                requiredMember: String
            }
            """.asSmithyModel()
        val codegenContext = serverTestCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider

        val unionShape = model.lookup<UnionShape>("test#Union")

        val project = TestWorkspace.testProject(symbolProvider)

        project.withModule(ServerRustModule.Model) {
            model.lookup<StructureShape>("test#Structure").serverRenderWithModelBuilder(
                project,
                model,
                symbolProvider,
                this,
                ServerRestJsonProtocol(codegenContext),
            )
        }

        project.withModule(ServerRustModule.Model) {
            UnionGenerator(model, symbolProvider, this, unionShape, renderUnknownVariant = false).render()
        }

        project.withModule(ServerRustModule.UnconstrainedModule) unconstrainedModuleWriter@{
            TestUtility.generateIsDisplay().invoke(this)
            TestUtility.generateIsError().invoke(this)

            project.withModule(ServerRustModule.Model) modelsModuleWriter@{
                UnconstrainedUnionGenerator(codegenContext, project.createInlineModuleCreator(), this@modelsModuleWriter, unionShape, SmithyValidationExceptionConversionGenerator(codegenContext)).render()

                this@unconstrainedModuleWriter.unitTest(
                    name = "unconstrained_union_fail_to_constrain",
                    test = """
                        let builder = crate::model::Structure::builder();
                        let union_unconstrained = union_unconstrained::UnionUnconstrained::Structure(builder);

                        let expected_err = crate::model::union::ConstraintViolation::Structure(
                            crate::model::structure::ConstraintViolation::MissingRequiredMember,
                        );
                        let err = crate::model::Union::try_from(union_unconstrained).unwrap_err();
                        assert_eq!(
                            expected_err, err
                        );
                        is_display(&err);
                        is_error(&err);
                        assert_eq!(err.to_string(), "`required_member` was not provided but it is required when building `Structure`");
                    """,
                )

                this@unconstrainedModuleWriter.unitTest(
                    name = "unconstrained_union_succeed_to_constrain",
                    test = """
                        let builder = crate::model::Structure::builder().required_member(String::from("david"));
                        let union_unconstrained = union_unconstrained::UnionUnconstrained::Structure(builder);

                        let expected: crate::model::Union = crate::model::Union::Structure(crate::model::Structure {
                            required_member: String::from("david"),
                        });
                        let actual: crate::model::Union = crate::model::Union::try_from(union_unconstrained).unwrap();

                        assert_eq!(expected, actual);
                    """,
                )

                this@unconstrainedModuleWriter.unitTest(
                    name = "unconstrained_union_converts_into_constrained",
                    test = """
                        let builder = crate::model::Structure::builder();
                        let union_unconstrained = union_unconstrained::UnionUnconstrained::Structure(builder);

                        let _union: crate::constrained::MaybeConstrained<crate::model::Union> =
                            union_unconstrained.into();
                    """,
                )
            }
        }
        project.renderInlineMemoryModules()
        project.compileAndTest()
    }

    @Test
    fun `a constrained union with a member named error should compile`() {
        // A union member named `error` renders as an `Error` variant on the constrained union. In the
        // generated `impl TryFrom<...> for Union`, that variant makes the path `Self::Error` ambiguous
        // with the `TryFrom::Error` associated type, so the generator must name the constraint
        // violation type explicitly. Both `try_from` code paths are exercised: the member targeting a
        // structure resolves to a public constrained type, and the member targeting an unconstrained
        // list that can reach a constrained shape resolves to a `pub(crate)` constrained type.
        val model =
            """
            ${'$'}version: "2"
            namespace com.example
            use aws.protocols#restJson1
            use smithy.framework#ValidationException

            @restJson1
            service TestService {
                version: "0.1",
                operations: [TestOperation]
            }

            @http(uri: "/test", method: "POST")
            operation TestOperation {
                input := {
                    @required
                    unionWithErrorStructure: UnionWithErrorStructure

                    @required
                    unionWithErrorList: UnionWithErrorList
                }
                errors: [ValidationException]
            }

            union UnionWithErrorStructure {
                error: ConstrainedStructure
                other: String
            }

            union UnionWithErrorList {
                error: ConstrainedStructureList
                other: String
            }

            structure ConstrainedStructure {
                @required
                requiredMember: String
            }

            list ConstrainedStructureList {
                member: ConstrainedStructure
            }
            """.asSmithyModel()

        // Ensure the generated SDK compiles.
        serverIntegrationTest(model, testCoverage = HttpTestType.Default) { _, _ -> }
    }
}
