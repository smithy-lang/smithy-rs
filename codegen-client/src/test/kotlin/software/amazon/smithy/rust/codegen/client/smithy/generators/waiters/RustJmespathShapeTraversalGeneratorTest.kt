/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators.waiters

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.fail
import software.amazon.smithy.jmespath.JmespathExpression
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.replaceLifetimes
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape

class RustJmespathShapeTraversalGeneratorTest {
    private class TestCase(
        private val codegenContext: ClientCodegenContext,
        private val rustCrate: RustCrate,
    ) {
        private val model = codegenContext.model
        private val symbolProvider = codegenContext.symbolProvider

        private val inputShape =
            model.lookup<OperationShape>("test#TestOperation")
                .inputShape(model)
        private val outputShape =
            model.lookup<OperationShape>("test#TestOperation")
                .outputShape(model)
        val input = symbolProvider.toSymbol(inputShape)
        val output = symbolProvider.toSymbol(outputShape)
        val entityPrimitives = symbolProvider.toSymbol(model.lookup<StructureShape>("test#EntityPrimitives"))
        val entityLists = symbolProvider.toSymbol(model.lookup<StructureShape>("test#EntityLists"))
        val entityMaps = symbolProvider.toSymbol(model.lookup<StructureShape>("test#EntityMaps"))
        val enum = symbolProvider.toSymbol(model.lookup<Shape>("test#Enum"))
        val struct = symbolProvider.toSymbol(model.lookup<Shape>("test#Struct"))
        val subStruct = symbolProvider.toSymbol(model.lookup<Shape>("test#SubStruct"))
        val traversalContext = TraversalContext(retainOption = false)

        val testInputDataFn: RuntimeType
        val testOutputDataFn: RuntimeType

        init {
            testInputDataFn =
                RuntimeType.forInlineFun("test_input_data", ClientRustModule.root) {
                    rustTemplate(
                        """
                        ##[allow(dead_code)]
                        fn test_input_data() -> #{Input} {
                            #{Input}::builder()
                                .name("foobaz")
                                .true_bool(true)
                                .false_bool(false)
                                .build()
                                .unwrap()
                        }
                        """,
                        "Input" to input,
                    )
                }

            testOutputDataFn =
                RuntimeType.forInlineFun("test_output_data", ClientRustModule.root) {
                    rustTemplate(
                        """
                        ##[allow(dead_code)]
                        fn test_output_data() -> #{Output} {
                            let primitives = #{EntityPrimitives}::builder()
                                .required_boolean(true)
                                .required_string("required-test")
                                .boolean(true)
                                .string("test")
                                .byte(1)
                                .short(2)
                                .integer(4)
                                .long(8)
                                .float(4.0)
                                .double(8.0)
                                .r##enum(#{Enum}::One)
                                .int_enum(1)
                                .build()
                                .unwrap();
                            #{Output}::builder()
                                .primitives(primitives.clone())
                                .lists(#{EntityLists}::builder()
                                    .booleans(true)
                                    .shorts(1).shorts(2)
                                    .integers(3).integers(4)
                                    .longs(5).longs(6)
                                    .floats(7.0).floats(8.0)
                                    .doubles(9.0).doubles(10.0)
                                    .strings("one").strings("two")
                                    .enums(#{Enum}::Two)
                                    .int_enums(2)
                                    .structs(#{Struct}::builder()
                                        .required_integer(1)
                                        .primitives(primitives.clone())
                                        .strings("lists_structs1_strings1")
                                        .sub_structs(#{SubStruct}::builder().sub_struct_primitives(primitives.clone()).build())
                                        .sub_structs(#{SubStruct}::builder().sub_struct_primitives(
                                            #{EntityPrimitives}::builder()
                                                .required_boolean(false)
                                                .required_string("why")
                                                .build()
                                                .unwrap())
                                            .build())
                                        .build()
                                        .unwrap())
                                    .structs(#{Struct}::builder()
                                        .required_integer(2)
                                        .integer(1)
                                        .string("foo")
                                        .build()
                                        .unwrap())
                                    .build())
                                .maps(#{EntityMaps}::builder()
                                    .strings("foo", "foo_oo")
                                    .strings("bar", "bar_ar")
                                    .booleans("foo", true)
                                    .booleans("bar", false)
                                    .structs("foo", #{Struct}::builder().required_integer(2).integer(5).strings("maps_foo_struct_strings1").build().unwrap())
                                    .structs("bar", #{Struct}::builder().required_integer(3).primitives(primitives).integer(7).build().unwrap())
                                    .build())
                                .build()
                        }
                        """,
                        "Output" to output,
                        "EntityPrimitives" to entityPrimitives,
                        "EntityLists" to entityLists,
                        "EntityMaps" to entityMaps,
                        "Enum" to enum,
                        "Struct" to struct,
                        "SubStruct" to subStruct,
                    )
                }
        }

        fun testCase(
            testName: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
            inputData: RuntimeType = testInputDataFn,
            outputData: RuntimeType = testOutputDataFn,
            dualBinding: Boolean = false,
        ) {
            val generator = RustJmespathShapeTraversalGenerator(codegenContext)
            val parsed = JmespathExpression.parse(expression)
            val bindings =
                when {
                    dualBinding ->
                        listOf(
                            TraversalBinding.Named("input", "_input", TraversedShape.from(model, inputShape)),
                            TraversalBinding.Named("output", "_output", TraversedShape.from(model, outputShape)),
                        )

                    else -> listOf(TraversalBinding.Global("_output", TraversedShape.from(model, outputShape)))
                }
            val generated = generator.generate(parsed, bindings, traversalContext)
            rustCrate.unitTest(testName) {
                rust("// jmespath: $expression")
                rust("// jmespath parsed: $parsed")
                rustBlockTemplate(
                    "fn inner<'a>(_input: &'a #{Input}, _output: &'a #{Output}) -> #{Option}<#{Ret}>",
                    "Input" to input,
                    "Output" to output,
                    "Ret" to generated.outputType.replaceLifetimes("a"),
                    *preludeScope,
                ) {
                    generated.output(this)
                    rustTemplate("#{Some}(${generated.identifier})", *preludeScope)
                }
                rustTemplate("let (input, output) = (#{in}(), #{out}());", "in" to inputData, "out" to outputData)
                rust("""println!("test data: {output:##?}");""")
                rust("""println!("jmespath: {}", ${expression.dq()});""")
                rust("let result = inner(&input, &output);")
                rust("""println!("result: {result:##?}");""")
                rust("let result = result.unwrap();")
                assertions()
                // Unused variable suppression
                rust("let _ = result;")
            }
        }

        fun invalid(
            expression: String,
            contains: String,
            dualBinding: Boolean = false,
        ) {
            try {
                val generator = RustJmespathShapeTraversalGenerator(codegenContext)
                val parsed = JmespathExpression.parse(expression)
                val bindings =
                    when {
                        dualBinding ->
                            listOf(
                                TraversalBinding.Named("input", "_input", TraversedShape.from(model, inputShape)),
                                TraversalBinding.Named("output", "_output", TraversedShape.from(model, outputShape)),
                            )

                        else -> listOf(TraversalBinding.Global("_output", TraversedShape.from(model, outputShape)))
                    }
                generator.generate(parsed, bindings, traversalContext).output(RustWriter.forModule("unsupported"))
                fail("expression '$expression' should have thrown InvalidJmesPathTraversalException")
            } catch (ex: InvalidJmesPathTraversalException) {
                ex.message shouldContain contains
            }
        }

        fun unsupported(
            expression: String,
            contains: String,
        ) {
            try {
                val generator = RustJmespathShapeTraversalGenerator(codegenContext)
                val parsed = JmespathExpression.parse(expression)
                generator.generate(
                    parsed,
                    listOf(TraversalBinding.Global("_output", TraversedShape.from(model, outputShape))),
                    traversalContext,
                ).output(RustWriter.forModule("unsupported"))
                fail("expression '$expression' should have thrown UnsupportedJmesPathException")
            } catch (ex: UnsupportedJmesPathException) {
                ex.message shouldContain contains
            }
        }
    }

    private fun integrationTest(testCases: TestCase.() -> Unit) {
        clientIntegrationTest(testModel()) { codegenContext, rustCrate ->
            TestCase(codegenContext, rustCrate).testCases()
        }
    }

    private fun simple(assertion: String): RustWriter.() -> Unit =
        {
            rust(assertion)
        }

    private val expectFalse = simple("assert_eq!(false, result);")
    private val expectTrue = simple("assert!(result);")
    private val itCompiles = simple("")

    @Test
    fun all() =
        integrationTest {
            fieldExpressions()
            subExpressions()
            namedBindings()
            flattenExpressions()
            literalTypes()
            functions()
            comparisons()
            objectProjections()
            filterProjections()
            booleanOperations()
            multiSelectLists()
            projectionFollowedByMultiSelectLists()
            complexCombinationsOfFeatures()

            unsupported("&('foo')", "Expression type expressions")
            unsupported("lists.integers[0]", "Index expressions")
            unsupported("""{"foo": primitives, "bar": integer}""", "Multi-select hash expressions")
            unsupported("lists.integers[0:2]", "Slice expressions")
        }

    private fun TestCase.fieldExpressions() {
        fun test(
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_field_$expression", expression, assertions)

        test("primitives") {
            rust("assert!(std::ptr::eq(output.primitives.as_ref().unwrap(), result));")
            rust("""assert_eq!("test", result.string.as_deref().unwrap());""")
        }
        test("lists") {
            rust("assert!(std::ptr::eq(output.lists.as_ref().unwrap(), result));")
        }
        test("maps") {
            rust("assert!(std::ptr::eq(output.maps.as_ref().unwrap(), result));")
        }

        invalid("doesNotExist", "Member `doesNotExist` doesn't exist")
    }

    private fun TestCase.subExpressions() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_subexpression_$name", expression, assertions)

        test("boolean", "primitives.boolean", expectTrue)
        test("string", "primitives.string", simple("assert_eq!(\"test\", result);"))
        test("byte", "primitives.byte", simple("assert_eq!(1i8, *result);"))
        test("short", "primitives.short", simple("assert_eq!(2i16, *result);"))
        test("integer", "primitives.integer", simple("assert_eq!(4i32, *result);"))
        test("long", "primitives.long", simple("assert_eq!(8i64, *result);"))
        test("float", "primitives.float", simple("assert_eq!(4f32, *result);"))
        test("double", "primitives.double", simple("assert_eq!(8f64, *result);"))
        test("enum", "primitives.enum") {
            rust("assert_eq!(#T::One, *result);", enum)
        }
        test("int_enum", "primitives.intEnum", simple("assert_eq!(1, *result);"))

        invalid("primitives.integer.foo", "Cannot look up fields in non-struct shapes")

        test("required_boolean", "primitives.requiredBoolean", expectTrue)
        test("required_string", "primitives.requiredString", simple("assert_eq!(\"required-test\", result);"))
    }

    private fun TestCase.namedBindings() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("named_bindings_$name", expression, assertions, dualBinding = true)

        test("input_and_output_bool_true", "input.trueBool && output.primitives.boolean", expectTrue)
        test("input_and_output_bool_false", "input.falseBool && output.primitives.boolean", expectFalse)

        invalid(
            "input.doesNotExist && output.primitives.boolean",
            "Member `doesNotExist` doesn't exist",
            dualBinding = true,
        )
    }

    private fun TestCase.projectionFollowedByMultiSelectLists() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_$name", expression, assertions)

        // Each struct in projection sets at least one of the selected fields, e.g. either `string` or `primitives.string` is `Some`.
        test("wildcard_projection_followed_by_multiselectlists", "lists.structs[*].[string, primitives.string][]") {
            rust("""assert_eq!(vec!["test", "foo"], result);""")
        }

        // The `primitives` field is `None` in structs obtained via `lists.structs[?string == 'foo']`
        test("filter_projection_followed_by_multiselectlists_empty", "lists.structs[?string == 'foo'].[primitives.string, primitives.requiredString][]") {
            rust("assert!(result.is_empty());")
        }

        // Unlike the previous, the `integer` field is set in a struct in the projection.
        test("filter_projection_followed_by_multiselectlists", "lists.structs[?string == 'foo'].[integer, primitives.integer][]") {
            rust("assert_eq!(vec![&1], result);")
        }

        test("object_projection_followed_by_multiselectlists", "maps.structs.*.[integer, primitives.integer][]") {
            rust("let mut result = result;")
            rust("result.sort();")
            rust("assert_eq!(vec![&4, &5, &7], result);")
        }
    }

    private fun TestCase.flattenExpressions() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_flatten_$name", expression, assertions)

        test("shortcircuit", "lists.structs[]") {
            rust("assert!(std::ptr::eq(output.lists.as_ref().unwrap().structs.as_ref().unwrap(), result));")
        }
        test("no_shortcircuit", "lists.structs[].primitives.string") {
            rust("assert_eq!(1, result.len());")
            rust("assert_eq!(\"test\", result[0]);")
        }
        test("no_shortcircuit_continued", "lists.structs[].strings") {
            rust("assert_eq!(1, result.len());")
            rust("assert_eq!(\"lists_structs1_strings1\", result[0]);")
        }
        test("nested_flattens", "lists.structs[].subStructs[].subStructPrimitives.string") {
            // it should compile
        }

        invalid("primitives.integer[]", "Left side of the flatten")
    }

    private fun TestCase.literalTypes() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_literal_$name", expression, assertions)

        test("bool", "`true`", expectTrue)
        test("int", "`0`", simple("assert_eq!(0f64, *result);"))
        test("float", "`1.5`", simple("assert_eq!(1.5f64, *result);"))
        test("string", "`\"foo\"`", simple("assert_eq!(\"foo\", result);"))

        unsupported("`null`", "Literal nulls")
        unsupported("`{}`", "Literal expression '`{}`'")
        unsupported("`[]`", "Literal expression '`[]`'")
    }

    private fun TestCase.functions() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_fn_$name", expression, assertions)

        test("list_length", "length(lists.structs[])", simple("assert_eq!(2, result);"))
        test("string_length", "length(primitives.string)", simple("assert_eq!(4, result);"))

        test("string_contains_false", "contains(primitives.string, 'foo')", expectFalse)
        test("string_contains_true", "contains(primitives.string, 'st')", expectTrue)

        test("strings_contains_false", "contains(lists.strings, 'foo')", expectFalse)
        test("strings_contains_true", "contains(lists.strings, 'two')", expectTrue)

        test("bools_contains_false", "contains(lists.booleans, `false`)", expectFalse)
        test("bools_contains_true", "contains(lists.booleans, `true`)", expectTrue)

        test("i16s_contains_false", "contains(lists.shorts, `0`)", expectFalse)
        test("i16s_contains_true", "contains(lists.shorts, `1`)", expectTrue)

        test("i32s_contains_false", "contains(lists.integers, `0`)", expectFalse)
        test("i32s_contains_true", "contains(lists.integers, `3`)", expectTrue)

        test("i64s_contains_false", "contains(lists.longs, `0`)", expectFalse)
        test("i64s_contains_true", "contains(lists.longs, `5`)", expectTrue)

        test("f32s_contains_false", "contains(lists.floats, `0`)", expectFalse)
        test("f32s_contains_true", "contains(lists.floats, `7.0`)", expectTrue)

        test("f64s_contains_false", "contains(lists.doubles, `0`)", expectFalse)
        test("f64s_contains_true", "contains(lists.doubles, `9.0`)", expectTrue)

        test("enums_contains_false", "contains(lists.enums, 'one')", expectFalse)
        test("enums_contains_true", "contains(lists.enums, 'two')", expectTrue)

        test("intenums_contains_false", "contains(lists.intEnums, `1`)", expectFalse)
        test("intenums_contains_true", "contains(lists.intEnums, `2`)", expectTrue)

        test("stringlit_contains_stringlit_false", "contains('foo', 'o0')", expectFalse)
        test("stringlit_contains_stringlit_true", "contains('foo', 'oo')", expectTrue)

        test("strings_contains_string", "contains(lists.strings, primitives.string)", expectFalse)
        test("i32s_contains_i32", "contains(lists.integers, primitives.integer)", expectTrue)
        test("i32s_contains_i16", "contains(lists.integers, primitives.short)", expectFalse)
        test("f32s_contains_f32", "contains(lists.floats, primitives.float)", expectFalse)

        test(
            "keys_struct", "keys(maps)",
            simple(
                "assert_eq!(6, result.len());" +
                    "assert!(result.contains(&\"booleans\".to_string()));" +
                    "assert!(result.contains(&\"strings\".to_string()));" +
                    "assert!(result.contains(&\"integers\".to_string()));" +
                    "assert!(result.contains(&\"enums\".to_string()));" +
                    "assert!(result.contains(&\"intEnums\".to_string()));" +
                    "assert!(result.contains(&\"structs\".to_string()));",
            ),
        )
        test(
            "keys_map", "keys(maps.strings)",
            simple(
                "assert_eq!(2, result.len());" +
                    "assert!(result.contains(&\"foo\".to_string()));" +
                    "assert!(result.contains(&\"bar\".to_string()));",
            ),
        )

        invalid("length()", "Length function takes exactly one argument")
        invalid("length(primitives.integer)", "Argument to `length` function")
        invalid("contains('foo')", "Contains function takes exactly two arguments")
        invalid("contains(primitives.integer, 'foo')", "First argument to `contains`")
        invalid("keys()", "Keys function takes exactly one argument")
        invalid("keys(primitives.integer)", "Argument to `keys` function")
        unsupported("contains(lists.structs, `null`)", "Checking for null with `contains`")
        unsupported("contains(lists.structs, lists)", "Checking for anything other than")
        unsupported("abs(`-1`)", "The `abs` function is not supported")
        unsupported("contains(lists.floats, primitives.string)", "Comparison of &f32 with &::std::string::String")
    }

    private fun TestCase.comparisons() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_compare_$name", expression, assertions)

        test("eq_boollit_w_boollit", "`true` == `true`", expectTrue)
        test("neq_boollit_w_boollit", "`true` != `true`", expectFalse)
        test("boollit_w_boollit", "`true` != `true`", expectFalse)
        test("bool_w_boollit", "primitives.boolean != `true`", expectFalse)
        test("bool_w_bool", "primitives.boolean == primitives.boolean", expectTrue)
        test("eq_integerlit_w_integerlit", "`0` == `0`", expectTrue)
        test("neq_integerlit_w_integerlit", "`0` != `0`", expectFalse)
        test("lt_integerlit_w_integerlit_false", "`0` < `0`", expectFalse)
        test("lt_integerlit_w_integerlit_true", "`0` < `1`", expectTrue)
        test("integer_w_integerlit", "primitives.integer != `0`", expectTrue)
        test("integer_w_integer", "primitives.integer == primitives.integer", expectTrue)
        test("float_w_integer_true", "primitives.float == primitives.integer", expectTrue)
        test("integer_w_float_true", "primitives.integer == primitives.float", expectTrue)
        test("float_w_integer_false", "primitives.float != primitives.integer", expectFalse)
        test("integer_w_float_false", "primitives.integer != primitives.float", expectFalse)
        test("eq_stringlit_w_stringlit", "'foo' == 'foo'", expectTrue)
        test("neq_stringlit_w_stringlit", "'bar' != 'foo'", expectTrue)
        test("string_w_stringlit_false", "primitives.string == 'foo'", expectFalse)
        test("string_w_stringlit_true", "primitives.string == 'test'", expectTrue)
        test("string_w_string", "primitives.string == primitives.string", expectTrue)
        test("enum_w_stringlit_false", "primitives.enum == 'one'", expectTrue)
        test("enum_w_stringlit_true", "primitives.enum == 'two'", expectFalse)
        test("enum_w_string", "primitives.enum == primitives.string", expectFalse)
        test("fn_w_number", "length(lists.structs[]) > `0`", expectTrue)

        unsupported("'foo' == `1`", "Comparison of &str with &f64")
        unsupported("primitives.string == primitives.integer", "Comparison of &::std::string::String with &i32")
    }

    private fun TestCase.objectProjections() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_obj_projection_$name", expression, assertions)

        test("simple", "maps.booleans.*") {
            rust("assert_eq!(2, result.len());")
            // Order is non-deterministic because we're getting the values of a hash map
            rust("assert_eq!(1, result.iter().filter(|&&&b| b == true).count());")
            rust("assert_eq!(1, result.iter().filter(|&&&b| b == false).count());")
        }
        test("continued", "maps.structs.*.integer") {
            rust("let mut result = result;")
            rust("result.sort();")
            rust("assert_eq!(vec![&5, &7], result);")
        }
        test("followed_by_optional_array", "maps.structs.*.strings") {
            rust("assert_eq!(vec![\"maps_foo_struct_strings1\"], result);")
        }
        test("w_function", "length(maps.structs.*.strings) == `1`", expectTrue)

        // Derived from https://github.com/awslabs/aws-sdk-rust/blob/8848f51e58fead8d230a0c15f0434b2812825c38/aws-models/ses.json#L2985
        test("followed_by_required_field", "maps.structs.*.requiredInteger") {
            rust("let mut result = result;")
            rust("result.sort();")
            rust("assert_eq!(vec![&2, &3], result);")
        }

        unsupported("primitives.integer.*", "Object projection is only supported on map types")
        unsupported("lists.structs[?`true`].*", "Object projection cannot be done on computed maps")
    }

    private fun TestCase.filterProjections() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_filter_projection_$name", expression, assertions)

        test("boollit", "lists.structs[?`true`]") {
            rust("assert_eq!(2, result.len());")
        }
        test("intcmp", "lists.structs[?primitives.integer > `0`]") {
            rust("assert_eq!(1, result.len());")
        }
        test("boollit_continued_empty", "lists.structs[?`true`].integer") {
            rust("assert_eq!(1, result.len());")
        }
        test("boollit_continued", "lists.structs[?`true`].primitives.integer") {
            rust("assert_eq!(1, result.len());")
        }
        test("intcmp_continued", "lists.structs[?primitives.integer > `0`].primitives.integer") {
            rust("assert_eq!(1, result.len());")
            rust("assert_eq!(4, **result.get(0).unwrap());")
        }
        test("intcmp_continued_filtered", "lists.structs[?primitives.integer == `0`].primitives.integer") {
            rust("assert_eq!(0, result.len());")
        }

        unsupported("primitives.integer[?`true`]", "Filter projections can only be done on lists")
        invalid("lists.structs[?`5`]", "The filter expression comparison must result in a bool")
    }

    private fun TestCase.booleanOperations() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_boolean_ops_$name", expression, assertions)

        test("lit_not", "!`true`", expectFalse)
        test("bool_not", "!(primitives.boolean)", expectFalse)
        test("lit_and_lit", "`true` && `false`", expectFalse)
        test("lit_or_lit", "`true` || `false`", expectTrue)
        test("bool_and_lit", "primitives.boolean && `true`", expectTrue)
        test("bool_or_lit", "primitives.boolean || `false`", expectTrue)
        test("bool_and_bool", "primitives.boolean && primitives.boolean", expectTrue)
        test("bool_or_bool", "primitives.boolean || primitives.boolean", expectTrue)
        test("paren_expressions", "(`true` || `false`) && `true`", expectTrue)

        unsupported("`5` || `true`", "Applying the `||` operation doesn't support non-bool")
        unsupported("`5` && `true`", "Applying the `&&` operation doesn't support non-bool")
        unsupported("!`5`", "Negation of a non-boolean type")
    }

    private fun TestCase.multiSelectLists() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_multiselectlists_$name", expression, assertions)

        test("intlist_contains", "contains([`1`, `2`, `3`], `1`)", expectTrue)
        test("stringlist_contains", "contains(['foo', 'bar'], 'foo')", expectTrue)
        test("primitive_int_list_contains", "contains(primitives.[integer, integer], primitives.integer)", expectTrue)
        test("primitive_bools", "primitives.[boolean, boolean]") {
            rust("assert_eq!(2, result.len());")
            rust("assert!(*result[0]);")
            rust("assert!(*result[1]);")
        }
        test("primitive_strings_contain", "contains(primitives.[string, string], primitives.string)", expectTrue)
    }

    private fun TestCase.complexCombinationsOfFeatures() {
        fun test(
            name: String,
            expression: String,
            assertions: RustWriter.() -> Unit,
        ) = testCase("traverse_complex_combos_$name", expression, assertions)

        test(
            "1",
            "(length(lists.structs[?!(integer < `0`) && integer >= `0` || `false`]) == `5`) == contains(lists.integers, length(maps.structs.*.strings))",
            itCompiles,
        )

        // Derived from https://github.com/awslabs/aws-sdk-rust/blob/8848f51e58fead8d230a0c15f0434b2812825c38/aws-models/auto-scaling.json#L4202
        // The first argument to `contains` evaluates to `Some([true])` since `length(...)` is 1 and `requiredInteger` in that struct is 1.
        test(
            "2",
            "contains(lists.structs[].[length(subStructs[?subStructPrimitives.requiredString=='why']) >= requiredInteger][], `true`)",
            expectTrue,
        )
    }

    private fun testModel() =
        """
        ${'$'}version: "2"
        namespace test

        @aws.protocols#awsJson1_0
        service TestService {
            operations: [TestOperation],
        }

        operation TestOperation {
            input: GetEntityRequest,
            output: GetEntityResponse,
            errors: [],
        }

        structure GetEntityRequest {
            @required
            name: String
            trueBool: Boolean
            falseBool: Boolean
        }

        structure GetEntityResponse {
            primitives: EntityPrimitives,
            lists: EntityLists,
            maps: EntityMaps,
        }

        structure EntityPrimitives {
            boolean: Boolean,
            string: String,
            byte: Byte,
            short: Short,
            integer: Integer,
            long: Long,
            float: Float,
            double: Double,
            enum: Enum,
            intEnum: IntEnum,

            @required requiredBoolean: Boolean,
            @required requiredString: String,
        }

        structure EntityLists {
            booleans: BooleanList,
            strings: StringList,
            shorts: ShortList
            integers: IntegerList,
            longs: LongList
            floats: FloatList,
            doubles: DoubleList,
            enums: EnumList,
            intEnums: IntEnumList,
            structs: StructList,
        }

        structure EntityMaps {
            booleans: BooleanMap,
            strings: StringMap,
            integers: IntegerMap,
            enums: EnumMap,
            intEnums: IntEnumMap,
            structs: StructMap,
        }

        enum Enum {
            ONE = "one",
            TWO = "two",
        }

        intEnum IntEnum {
            ONE = 1,
            TWO = 2,
        }

        structure Struct {
            @required
            requiredInteger: Integer,
            primitives: EntityPrimitives,
            strings: StringList,
            integer: Integer,
            string: String,
            enums: EnumList,
            subStructs: SubStructList,
        }

        structure SubStruct {
            subStructPrimitives: EntityPrimitives,
        }

        list BooleanList { member: Boolean }
        list StringList { member: String }
        list ShortList { member: Short }
        list IntegerList { member: Integer }
        list LongList { member: Long }
        list FloatList { member: Float }
        list DoubleList { member: Double }
        list EnumList { member: Enum }
        list IntEnumList { member: IntEnum }
        list StructList { member: Struct }
        list SubStructList { member: SubStruct }
        map BooleanMap { key: String, value: Boolean }
        map StringMap { key: String, value: String }
        map IntegerMap { key: String, value: Integer }
        map EnumMap { key: String, value: Enum }
        map IntEnumMap { key: String, value: IntEnum }
        map StructMap { key: String, value: Struct }
        """.asSmithyModel()
}
