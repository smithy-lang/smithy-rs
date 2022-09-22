/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.parse

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.rustlang.RustModule
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.client.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.client.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.client.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.client.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.client.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.client.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.client.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape

class AwsQueryParserGeneratorTest {
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
        val model = RecursiveShapeBoxer.transform(OperationNormalizer.transform(baseModel))
        val symbolProvider = testSymbolProvider(model)
        val parserGenerator = AwsQueryParserGenerator(
            testCodegenContext(model),
            RuntimeType.wrappedXmlErrors(TestRuntimeConfig),
        )
        val operationParser = parserGenerator.operationParser(model.lookup("test#SomeOperation"))!!
        val project = TestWorkspace.testProject(testSymbolProvider(model))

        project.lib { writer ->
            writer.unitTest(
                name = "valid_input",
                test = """
                    let xml = br#"
                    <SomeOperationResponse>
                        <SomeOperationResult someAttribute="5">
                            <someVal>Some value</someVal>
                        </SomeOperationResult>
                    </someOperationResponse>
                    "#;
                    let output = ${writer.format(operationParser)}(xml, output::some_operation_output::Builder::default()).unwrap().build();
                    assert_eq!(output.some_attribute, Some(5));
                    assert_eq!(output.some_val, Some("Some value".to_string()));
                """,
            )
        }

        project.withModule(RustModule.public("model")) {
            model.lookup<StructureShape>("test#SomeOutput").renderWithModelBuilder(model, symbolProvider, it)
        }

        project.withModule(RustModule.public("output")) {
            model.lookup<OperationShape>("test#SomeOperation").outputShape(model)
                .renderWithModelBuilder(model, symbolProvider, it)
        }
        project.compileAndTest()
    }
}
