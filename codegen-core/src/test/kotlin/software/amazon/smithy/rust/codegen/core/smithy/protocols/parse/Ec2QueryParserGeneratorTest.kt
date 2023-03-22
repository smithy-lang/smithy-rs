/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape

class Ec2QueryParserGeneratorTest {
    private val baseModel = """
        namespace test
        use aws.protocols#awsQuery

        structure SomeOutput {
            @xmlAttribute
            someAttribute: Long,

            someVal: String
        }

        operation SomeOperation {
            output: SomeOutput
        }
    """.asSmithyModel()

    @Test
    fun `it modifies operation parsing to include Response and Result tags`() {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator = Ec2QueryParserGenerator(
            codegenContext,
            RuntimeType.wrappedXmlErrors(TestRuntimeConfig),
        )
        val operationParser = parserGenerator.operationParser(model.lookup("test#SomeOperation"))!!
        val project = TestWorkspace.testProject(testSymbolProvider(model))

        project.lib {
            unitTest(
                "valid_input",
                """
                let xml = br#"
                <SomeOperationResponse someAttribute="5">
                    <someVal>Some value</someVal>
                </someOperationResponse>
                "#;
                let output = ${format(operationParser)}(xml, test_output::SomeOperationOutput::builder()).unwrap().build();
                assert_eq!(output.some_attribute, Some(5));
                assert_eq!(output.some_val, Some("Some value".to_string()));
                """,
            )
        }

        model.lookup<StructureShape>("test#SomeOutput").also { struct ->
            struct.renderWithModelBuilder(model, symbolProvider, project)
        }

        model.lookup<OperationShape>("test#SomeOperation").outputShape(model).also { output ->
            output.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }
}
