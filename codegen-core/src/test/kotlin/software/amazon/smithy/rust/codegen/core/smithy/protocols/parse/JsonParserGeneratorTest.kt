/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.TestEnumType
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.core.smithy.protocols.restJsonFieldName
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape

class JsonParserGeneratorTest {
    private val baseModel =
        """
        namespace test
        use aws.protocols#restJson1

        union Choice {
            blob: Blob,
            boolean: Boolean,
            date: Timestamp,
            document: Document,
            enum: FooEnum,
            int: Integer,
            list: SomeList,
            listSparse: SomeSparseList,
            long: Long,
            map: MyMap,
            mapSparse: MySparseMap,
            number: Double,
            s: String,
            top: Top,
            unit: Unit,
        }

        @enum([{name: "FOO", value: "FOO"}])
        string FooEnum

        map MyMap {
            key: String,
            value: Choice,
        }

        @sparse
        map MySparseMap {
            key: String,
            value: Choice,
        }

        list SomeList {
            member: Choice
        }

        @sparse
        list SomeSparseList {
            member: Choice
        }

        structure EmptyStruct {
        }

        structure Top {
            @required
            choice: Choice,
            field: String,
            extra: Integer,
            @jsonName("rec")
            recursive: TopList,
            empty: EmptyStruct,
        }

        list TopList {
            member: Top
        }

        structure OpOutput {
            @httpHeader("x-test")
            someHeader: String,

            top: Top
        }

        @error("client")
        structure Error {
            message: String,
            reason: String
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            output: OpOutput,
            errors: [Error]
        }
        """.asSmithyModel()

    @Test
    fun `generates valid deserializers`() {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider

        val parserGenerator =
            JsonParserGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/json")),
                ::restJsonFieldName,
            )
        val operationGenerator = parserGenerator.operationParser(model.lookup("test#Op"))
        val payloadGenerator = parserGenerator.payloadParser(model.lookup("test#OpOutput\$top"))
        val errorParser = parserGenerator.errorParser(model.lookup("test#Error"))

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib {
            unitTest(
                "json_parser",
                """
                use test_model::Choice;

                // Generate the document serializer even though it's not tested directly
                // ${format(payloadGenerator)}

                let json = br#"
                    { "top":
                        { "extra": 45,
                          "field": "something",
                          "choice": { "int": 5 },
                          "empty": { "not_empty": true }
                        }
                    }
                "#;

                let output = ${format(operationGenerator!!)}(json, test_output::OpOutput::builder()).unwrap().build();
                let top = output.top.expect("top");
                assert_eq!(Some(45), top.extra);
                assert_eq!(Some("something".to_string()), top.field);
                assert_eq!(Choice::Int(5), top.choice);
                """,
            )
            unitTest(
                "empty_body",
                """
                // empty body
                let output = ${format(operationGenerator)}(b"", test_output::OpOutput::builder()).unwrap().build();
                assert_eq!(output.top, None);
                """,
            )
            unitTest(
                "unknown_variant",
                """
                // unknown variant
                let input = br#"{ "top": { "choice": { "somenewvariant": "data" } } }"#;
                let output = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).unwrap().build();
                assert!(output.top.unwrap().choice.is_unknown());
                """,
            )

            unitTest(
                "dunder_type_should_be_ignored",
                """
                // __type field should be ignored during deserialization
                let input = br#"{ "top": { "choice": { "int": 5, "__type": "value-should-be-ignored-anyway" } } }"#;
                let output = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).unwrap().build();
                use test_model::Choice;
                assert_eq!(Choice::Int(5), output.top.unwrap().choice);
                """,
            )

            unitTest(
                "allow_null_for_variants",
                """
                // __type field should be ignored during deserialization
                let input = br#"{ "top": { "choice": { "blob": null, "boolean": null, "int": 5, "long": null, "__type": "value-should-be-ignored-anyway" } } }"#;
                let output = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).unwrap().build();
                use test_model::Choice;
                assert_eq!(Choice::Int(5), output.top.unwrap().choice);
                """,
            )

            unitTest(
                "all_variants_null",
                """
                // __type field should be ignored during deserialization
                let input = br#"{ "top": { "choice": { "blob": null, "boolean": null, "int": null, "long": null, "__type": "value-should-be-ignored-anyway" } } }"#;
                let _err = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).expect_err("invalid union");
                """,
            )

            unitTest(
                "empty_error",
                """
                // empty error
                let error_output = ${format(errorParser!!)}(b"", test_error::Error::builder()).unwrap().build();
                assert_eq!(error_output.message, None);
                """,
            )

            unitTest(
                "error_with_message",
                """
                // error with message
                let error_output = ${format(errorParser)}(br#"{"message": "hello"}"#, test_error::Error::builder()).unwrap().build();
                assert_eq!(error_output.message.expect("message should be set"), "hello");
                """,
            )

            unitTest(
                "dense_list_rejects_null",
                """
                // dense list should reject null values
                let input = br#"{ "top": { "choice": { "list": [{ "int": 5 }, null, { "int": 6 }] } } }"#;
                let err = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).expect_err("dense list cannot contain null");
                assert!(err.to_string().contains("dense list cannot contain null values"));
                """,
            )

            unitTest(
                "dense_map_rejects_null",
                """
                // dense map should reject null values
                let input = br#"{ "top": { "choice": { "map": { "a": { "int": 5 }, "b": null } } } }"#;
                let err = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).expect_err("dense map cannot contain null");
                assert!(err.to_string().contains("dense map cannot contain null values"));
                """,
            )

            unitTest(
                "sparse_list_allows_null",
                """
                // sparse list should allow null values
                let input = br#"{ "top": { "choice": { "listSparse": [{ "int": 5 }, null, { "int": 6 }] } } }"#;
                let output = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).unwrap().build();
                use test_model::Choice;
                match output.top.unwrap().choice {
                    Choice::ListSparse(list) => {
                        assert_eq!(list.len(), 3);
                        assert_eq!(list[0], Some(Choice::Int(5)));
                        assert_eq!(list[1], None);
                        assert_eq!(list[2], Some(Choice::Int(6)));
                    }
                    _ => panic!("expected ListSparse variant"),
                }
                """,
            )

            unitTest(
                "sparse_map_allows_null",
                """
                // sparse map should allow null values
                let input = br#"{ "top": { "choice": { "mapSparse": { "a": { "int": 5 }, "b": null } } } }"#;
                let output = ${format(operationGenerator)}(input, test_output::OpOutput::builder()).unwrap().build();
                use test_model::Choice;
                match output.top.unwrap().choice {
                    Choice::MapSparse(map) => {
                        assert_eq!(map.len(), 2);
                        assert_eq!(map.get("a"), Some(&Some(Choice::Int(5))));
                        assert_eq!(map.get("b"), Some(&None));
                    }
                    _ => panic!("expected MapSparse variant"),
                }
                """,
            )
        }
        model.lookup<StructureShape>("test#Top").also { top ->
            top.renderWithModelBuilder(model, symbolProvider, project)
            model.lookup<StructureShape>("test#EmptyStruct").renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(top) {
                UnionGenerator(model, symbolProvider, this, model.lookup("test#Choice")).render()
                val enum = model.lookup<StringShape>("test#FooEnum")
                EnumGenerator(model, symbolProvider, enum, TestEnumType, emptyList()).render(this)
            }
        }
        model.lookup<OperationShape>("test#Op").outputShape(model).also { output ->
            output.renderWithModelBuilder(model, symbolProvider, project)
        }
        model.lookup<StructureShape>("test#Error").also { error ->
            error.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }
}
