/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.TestEnumType
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
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

internal class XmlBindingTraitParserGeneratorTest {
    private val baseModel =
        """
        namespace test
        use aws.protocols#restXml
        union Choice {
            @xmlFlattened
            @xmlName("Hi")
            flatMap: MyMap,

            deepMap: MyMap,

            @xmlFlattened
            flatList: SomeList,

            deepList: SomeList,

            s: String,

            enum: FooEnum,

            date: Timestamp,

            number: Double,

            top: Top,

            blob: Blob,

            unit: Unit,
        }

        @enum([{name: "FOO", value: "FOO"}])
        string FooEnum

        map MyMap {
            @xmlName("Name")
            key: String,

            @xmlName("Setting")
            value: Choice,
        }

        list SomeList {
            member: Choice
        }

        structure Top {
            choice: Choice,

            @xmlAttribute
            extra: Long,

            @xmlName("prefix:local")
            renamedWithPrefix: String,
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            input: Top,
            output: Top
        }
        """.asSmithyModel()

    @Test
    fun `generates valid parsers`() {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator =
            XmlBindingTraitParserGenerator(
                codegenContext,
                RuntimeType.wrappedXmlErrors(TestRuntimeConfig),
            ) { _, inner -> inner("decoder") }
        val operationParser = parserGenerator.operationParser(model.lookup("test#Op"))!!

        val choiceShape = model.lookup<UnionShape>("test#Choice")
        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib {
            unitTest(name = "valid_input") {
                rustTemplate(
                    """
                    let xml = br##"<Top>
                        <choice>
                            <Hi>
                                <Name>some key</Name>
                                <Setting>
                                    <s>hello</s>
                                </Setting>
                            </Hi>
                        </choice>
                        <prefix:local>hey</prefix:local>
                    </Top>
                    "##;
                    let output = ${format(operationParser)}(xml, test_output::OpOutput::builder()).unwrap().build();
                    let mut map = std::collections::HashMap::new();
                    map.insert("some key".to_string(), #{Choice}::S("hello".to_string()));
                    assert_eq!(output.choice, Some(#{Choice}::FlatMap(map)));
                    assert_eq!(output.renamed_with_prefix.as_deref(), Some("hey"));
                    """,
                    "Choice" to symbolProvider.toSymbol(choiceShape),
                )
            }

            unitTest(name = "ignore_extras") {
                rustTemplate(
                    """
                    let xml = br##"<Top>
                        <notchoice>
                            <extra/>
                            <stuff/>
                            <noone/>
                            <needs>5</needs>
                        </notchoice>
                        <choice>
                            <Hi>
                                <Name>some key</Name>
                                <Setting>
                                    <s>hello</s>
                                </Setting>
                            </Hi>
                        </choice>
                    </Top>
                    "##;
                    let output = ${format(operationParser)}(xml, test_output::OpOutput::builder()).unwrap().build();
                    let mut map = std::collections::HashMap::new();
                    map.insert("some key".to_string(), #{Choice}::S("hello".to_string()));
                    assert_eq!(output.choice, Some(#{Choice}::FlatMap(map)));
                    """,
                    "Choice" to symbolProvider.toSymbol(choiceShape),
                )
            }

            unitTest(
                name = "nopanics_on_invalid",
                test = """
                    let xml = br#"<Top>
                        <notchoice>
                            <extra/>
                            <stuff/>
                            <noone/>
                            <needs>5</needs>
                        </notchoice>
                        <choice>
                            <Hey>
                                <Name>some key</Name>
                                <Setting>
                                    <s>hello</s>
                                </Setting>
                            </Hey>
                        </choice>
                    </Top>
                    "#;
                    ${format(operationParser)}(xml, test_output::OpOutput::builder()).expect("unknown union variant does not cause failure");
                """,
            )
            unitTest(
                name = "unknown_union_variant",
                test = """
                    let xml = br#"<Top>
                        <choice>
                            <NewVariantName>
                                <Name>some key</Name>
                                <Setting>
                                    <s>hello</s>
                                </Setting>
                            </NewVariantName>
                        </choice>
                    </Top>
                    "#;
                    let output = ${format(operationParser)}(xml, test_output::OpOutput::builder()).unwrap().build();
                    assert!(output.choice.unwrap().is_unknown());
                """,
            )
        }
        model.lookup<StructureShape>("test#Top").also { top ->
            top.renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(top) {
                UnionGenerator(model, symbolProvider, this, choiceShape).render()
                model.lookup<StringShape>("test#FooEnum").also { enum ->
                    EnumGenerator(model, symbolProvider, enum, TestEnumType, emptyList()).render(this)
                }
            }
        }

        model.lookup<OperationShape>("test#Op").outputShape(model).also { out ->
            out.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }
}
