/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup

class ErrorCorrectionTest {
    private val model = """
        namespace com.example
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }

        operation SayHello { input: TestInput }
        structure TestInput { nested: TestStruct }
        structure TestStruct {
           @required
           foo: String,
           @required
           byteValue: Byte,
           @required
           listValue: StringList,
           @required
           mapValue: ListMap,
           @required
           doubleListValue: DoubleList
           @required
           document: Document
           @required
           nested: Nested
           @required
           blob: Blob
           @required
           enum: Enum
           @required
           union: U
           notRequired: String
        }

        enum Enum {
            A,
            B,
            C
        }

        union U {
            A: Integer,
            B: String,
            C: Unit
        }

        structure Nested {
            @required
            a: String
        }

        list StringList {
            member: String
        }

        list DoubleList {
            member: StringList
        }

        map ListMap {
            key: String,
            value: StringList
        }
    """.asSmithyModel(smithyVersion = "2.0")

    @Test
    fun correctMissingFields() {
        val shape = model.lookup<StructureShape>("com.example#TestStruct")
        clientIntegrationTest(model) { ctx, crate ->
            crate.lib {
                val codegenCtx =
                    arrayOf("correct_errors" to ctx.correctErrors(shape)!!, "Shape" to ctx.symbolProvider.toSymbol(shape))
                rustTemplate(
                    """
                    /// avoid unused warnings
                pub fn use_fn_publicly() { #{correct_errors}(#{Shape}::builder()); } """,
                    *codegenCtx,
                )
                unitTest("test_default_builder") {
                    rustTemplate(
                        """
                        let builder = #{correct_errors}(#{Shape}::builder().foo("abcd"));
                        let shape = builder.build().unwrap();
                        // don't override a field already set
                        assert_eq!(shape.foo(), "abcd");
                        // set nested fields
                        assert_eq!(shape.nested().a(), "");
                        // don't default non-required fields
                        assert_eq!(shape.not_required(), None);

                        // set defaults for everything else
                        assert_eq!(shape.blob().as_ref(), &[]);

                        assert!(shape.list_value().is_empty());
                        assert!(shape.map_value().is_empty());
                        assert!(shape.double_list_value().is_empty());

                        // enums and unions become unknown variants
                        assert!(matches!(shape.r##enum(), crate::types::Enum::Unknown(_)));
                        assert!(shape.union().is_unknown());
                        """,
                        *codegenCtx,
                    )
                }
            }
        }
    }
}
