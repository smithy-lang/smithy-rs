/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.server.smithy.ConstraintViolationSymbolProvider
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverRenderWithModelBuilder
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.util.lookup

class UnconstrainedCollectionGeneratorTest {
    private val model =
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
    private val symbolProvider = serverTestSymbolProvider(model)

    private val serviceShape = model.lookup<ServiceShape>("test#TestService")
    private val listA = model.lookup<ListShape>("test#ListA")
    private val listB = model.lookup<ListShape>("test#ListB")

    private val unconstrainedShapeSymbolProvider = UnconstrainedShapeSymbolProvider(symbolProvider, model, serviceShape)
    private val pubCrateConstrainedShapeSymbolProvider =
        PubCrateConstrainedShapeSymbolProvider(symbolProvider, model, serviceShape)

    private fun renderModel(): RustWriter {
        val writer = RustWriter.root()
        val modelsModuleWriter = writer.withModule(ModelsModule.name) {
            model.lookup<StructureShape>("test#StructureC").serverRenderWithModelBuilder(model, symbolProvider, this)
        }

        writer.withModule("constrained") {
            listOf(listA, listB).forEach { shape ->
                PubCrateConstrainedCollectionGenerator(
                    model,
                    symbolProvider,
                    unconstrainedShapeSymbolProvider,
                    pubCrateConstrainedShapeSymbolProvider,
                    this,
                    shape
                ).render()
            }
        }
        writer.withModule("unconstrained") {
            val unconstrainedModuleWriter = this

            val constraintViolationSymbolProvider =
                ConstraintViolationSymbolProvider(symbolProvider, model, serviceShape)
            listOf(listA, listB).forEach {
                UnconstrainedCollectionGenerator(
                    model,
                    symbolProvider,
                    unconstrainedShapeSymbolProvider,
                    pubCrateConstrainedShapeSymbolProvider,
                    constraintViolationSymbolProvider,
                    unconstrainedModuleWriter,
                    modelsModuleWriter,
                    it
                ).render()
            }
        }

        return writer
    }

    @Test
    fun `it should fail to constrain with the first error`() {
        val writer = renderModel()

        writer.compileAndTest(
            """
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
            """
        )
    }

    @Test
    fun `it should succeed to constrain`() {
        val writer = renderModel()

        writer.compileAndTest(
            """
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
            """
        )
    }

    @Test
    fun `it should convert into MaybeConstrained`() {
        val writer = renderModel()

        writer.compileAndTest(
            """
            let c_builder = crate::model::StructureC::builder();
            let list_b_unconstrained = list_b_unconstrained::ListBUnconstrained(vec![c_builder]);
            let list_a_unconstrained = list_a_unconstrained::ListAUnconstrained(vec![list_b_unconstrained]);

            let _list_a: crate::constrained::MaybeConstrained<crate::constrained::list_a_constrained::ListAConstrained> = list_a_unconstrained.into();
            """
        )
    }
}
