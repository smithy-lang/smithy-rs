package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordConfig
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructSettings
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.AddSyntheticTraitForImplDisplay
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.REDACTION
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.lookup
import java.io.File

class NestedErrorStructureTest {
    private var sampleModel = File("../codegen-core/common-test-models/nested-error.smithy").readText().asSmithyModel()
    private val model = sampleModel.let(AddSyntheticTraitForImplDisplay::transform)

    private val errorInInput = model.lookup<StructureShape>("sample#ErrorInInput")
    private val simpleError = model.lookup<StructureShape>("sample#SimpleError")
    private val errorWithDeepCompositeShape = model.lookup<StructureShape>("sample#ErrorWithDeepCompositeShape")
    private val composedSensitiveError = model.lookup<StructureShape>("sample#ComposedSensitiveError")
    private val errorWithNestedError = model.lookup<StructureShape>("sample#ErrorWithNestedError")
    private val errorMessage = model.lookup<StructureShape>("sample#ErrorMessage")
    private val wrappedErrorMessage = model.lookup<StructureShape>("sample#WrappedErrorMessage")
    private val sensitiveMessage = model.lookup<StructureShape>("sample#SensitiveMessage")

    private val allStructures =
        arrayOf(
            errorInInput,
            simpleError,
            errorWithDeepCompositeShape,
            composedSensitiveError,
            errorWithNestedError,
            errorMessage,
            wrappedErrorMessage,
            sensitiveMessage,
        )
    private val errorShapes =
        arrayOf(
            errorInInput,
            simpleError,
            errorWithDeepCompositeShape,
            errorWithNestedError,
            composedSensitiveError,
        )

    private val rustReservedWordConfig: RustReservedWordConfig =
        RustReservedWordConfig(
            structureMemberMap = StructureGenerator.structureMemberNameMap,
            enumMemberMap = emptyMap(),
            unionMemberMap = emptyMap(),
        )

    private val provider = testSymbolProvider(model, rustReservedWordConfig = rustReservedWordConfig)

    private fun structureGenerator(
        writer: RustWriter,
        shape: StructureShape,
    ) = StructureGenerator(model, provider, writer, shape, emptyList(), StructSettings(flattenVecAccessors = true))

    private fun errorImplGenerator(
        writer: RustWriter,
        shape: StructureShape,
    ) = ErrorImplGenerator(model, provider, writer, shape, shape.getTrait<ErrorTrait>()!!, emptyList())

    /**
     * Generates Rust code to create an ErrorMessage with specified fields and returns the expected formatting.
     *
     * @param statusCode The required status code value
     * @param errorMessage The required error message string
     * @param isRetryable The required boolean flag for retryability
     * @param optionalValues Map of optional field names to values (null indicates None)
     * @return Pair of (Rust initialization code, assert_eq! statement)
     */
    private fun generateErrorMessageWithAssert(
        statusCode: Int,
        errorMessage: String,
        isRetryable: Boolean,
        optionalValues: Map<String, Any?> = emptyMap()
    ): Pair<String, String> {
        // Generate the Rust initialization code
        val rustCode = generateErrorMessage(statusCode, errorMessage, isRetryable, optionalValues = optionalValues)

        // Build the expected output string for assert_eq!
        val expectedOutput = buildExpectedOutput(statusCode, errorMessage, isRetryable, optionalValues = optionalValues)

        // Generate the assert_eq! statement
        val assertStatement = """
            let formatted = format!("{message}");
            assert_eq!(formatted, "$expectedOutput");
            """

        return Pair(rustCode, assertStatement)
    }

    /**
     * Builds the expected string representation of the ErrorMessage
     */
    private fun buildExpectedOutput(
        statusCode: Int,
        errorMessage: String,
        isRetryable: Boolean,
        optionalValues: Map<String, Any?> = emptyMap()
    ): String {
        val parts = mutableListOf(
            "status_code=$statusCode",
            "error_message=$errorMessage",
            "is_retryable=$isRetryable"
        )
        val codeBuilder = StringBuilder(parts.joinToString(", "))
        codeBuilder.append(", ")

        // For assertions, just use quoted strings without .to_owned()
        fillOptionalValues(
            codeBuilder,
            assignmentOperator = "=",
            lineSeparator = ", ",
            optionalValues = optionalValues,
            stringFormatter = { "$it" } // Simple quoted string for assertions
        )

        val optional = codeBuilder.toString().dropLast(2)

        // Return the formatted string
        return "ErrorMessage {$optional}"
    }

    /**
     * Generates Rust code to create an ErrorMessage with specified fields.
     */
    private fun generateErrorMessage(
        statusCode: Int,
        errorMessage: String,
        isRetryable: Boolean,
        prefix: String = "let message = ",
        suffix: String = ";",
        optionalValues: Map<String, Any?> = emptyMap()
    ): String {
        val codeBuilder = StringBuilder("$prefix crate::test_model::ErrorMessage {\n")

        // Add required fields
        codeBuilder.append("    status_code: $statusCode,\n")
        codeBuilder.append("    error_message: \"$errorMessage\".to_owned(),\n")
        codeBuilder.append("    is_retryable: $isRetryable,\n")

        // Add optional fields with indentation and proper syntax for code generation
        codeBuilder.append("    ")
        fillOptionalValues(
            codeBuilder,
            assignmentOperator = ": ",
            lineSeparator = ",\n    ",
            optionalValues = optionalValues,
            stringFormatter = { "\"$it\".to_owned()" } // Use .to_owned() for code generation
        )

        codeBuilder.append("\n} $suffix")
        return codeBuilder.toString()
    }

    /**
     * Fills optional values in the provided StringBuilder
     *
     * @param codeBuilder StringBuilder to append formatted values to
     * @param assignmentOperator Operator for assignment (e.g., ":" or "=")
     * @param lineSeparator Separator between lines (e.g., ",\n" or ", ")
     * @param optionalValues Map of field names to values
     * @param stringFormatter Lambda for formatting string values
     */
    private fun fillOptionalValues(
        codeBuilder: StringBuilder,
        assignmentOperator: String = ":",
        lineSeparator: String = ",\n",
        optionalValues: Map<String, Any?> = emptyMap(),
        stringFormatter: (String) -> String = { "\"$it\".to_owned()" } // Default uses .to_owned()
    ) {
        // List of all optional fields
        val allOptionalFields = listOf(
            "request_id",
            "time_stamp",
            "ratio",
            "precision",
            "data_size",
            "byte_count",
            "flags",
            "document_data",
            "blob_data",
            "tags",
            "error_codes",
        )

        // Add all optional fields, using provided values or None
        for (field in allOptionalFields) {
            val value = optionalValues[field]
            val formattedValue = formatOptionalValue(field, value, stringFormatter)
            codeBuilder.append("$field$assignmentOperator$formattedValue$lineSeparator")
        }
    }

    /**
     * Formats an optional value as Some(value) or None
     *
     * @param field Field name
     * @param value Field value
     * @param stringFormatter Lambda for formatting string values
     * @return Formatted value string
     */
    private fun formatOptionalValue(
        field: String,
        value: Any?,
        stringFormatter: (String) -> String
    ): String {
        return if (value == null) {
            "None"
        } else when (field) {
            "request_id" -> "Some(${stringFormatter(value.toString())})"
            "time_stamp" -> {
                if (value is String) {
                    "Some(aws_smithy_types::DateTime::from_str(\"$value\", aws_smithy_types::date_time::Format::DateTime).unwrap())"
                } else {
                    "Some(aws_smithy_types::DateTime::from_secs($value))"
                }
            }
            "document_data" -> {
                if (value is String) {
                    "Some(aws_smithy_json::Value::from_str(\"$value\").unwrap())"
                } else {
                    "Some($value)"
                }
            }
            "blob_data" -> "Some(aws_smithy_types::Blob::new($value))"
            "tags" -> {
                if (value is Map<*, *>) {
                    val mapEntries = value.entries.joinToString(", ") { (k, v) ->
                        "${stringFormatter(k.toString())} => ${stringFormatter(v.toString())}"
                    }
                    "Some(std::collections::HashMap::from([$mapEntries]))"
                } else {
                    "Some($value)"
                }
            }
            "error_codes" -> {
                if (value is List<*>) {
                    val items = value.joinToString(", ")
                    "Some(vec![$items])"
                } else {
                    "Some($value)"
                }
            }
            else -> {
                if (value is String) {
                    "Some(${stringFormatter(value)})"
                } else {
                    "Some($value)"
                }
            }
        }
    }

    @Test
    fun `generate nested error structure`() {
        val project = TestWorkspace.testProject(provider)
        // Generate code for each structure.
        for (shape in allStructures) {
            project.useShapeWriter(shape) {
                structureGenerator(this, shape).render()
            }
        }
        // Generate code for each structure marked with an error trait.
        for (shape in errorShapes) {
            project.useShapeWriter(shape) {
                errorImplGenerator(this, shape).render()
            }
        }

        project.withModule(
            RustModule.public("tests"),
        ) {
            unitTest("optional_field_prints_none") {
                val (rustCode, assertStatement) = generateErrorMessageWithAssert(
                    statusCode = 333,
                    errorMessage = "this is an error",
                    isRetryable = false,
                    optionalValues = mapOf("request_id" to null)
                )

                rustTemplate("""
                    $rustCode
                    $assertStatement
                """)
            }

            unitTest("optional_field_prints_value") {
                val (rustCode, assertStatement) = generateErrorMessageWithAssert(
                    statusCode = 419,
                    errorMessage = "this is an error",
                    isRetryable = true,
                    optionalValues = mapOf("request_id" to "1234")
                )

                rustTemplate(
                    """
                    $rustCode
                    $assertStatement
                """
                )
            }

            unitTest("sensitive_is_redacted") {
                val redacted = REDACTION.removeSurrounding("\"")
                rustTemplate(
                    """
                        let message = crate::test_model::SensitiveMessage {
                            nothing: Some("some value".to_owned()),
                            should: Some("some other value".to_owned()),
                            be_printed: Some("another value".to_owned()),
                        };
                        let formatted = format!("{message}");
                        assert_eq!(formatted, "SensitiveMessage {nothing=$redacted, should=$redacted, be_printed=$redacted}");
                        """,
                )
            }
            unitTest("nested_error_structure_do_not_implement_display_twice") {
                val rustCode = generateErrorMessage(509, "this is an error", false, prefix = "", suffix = "")
                val expectedOutput = buildExpectedOutput(509, "this is an error", false)

                rustTemplate(
                    """
                    let message = crate::test_error::ErrorWithNestedError {
                        message: Some(crate::test_error::ErrorWithDeepCompositeShape {
                            message: Some(crate::test_model::WrappedErrorMessage {
                                some_value: Some(123),
                                contained: Some($rustCode),
                            }),
                        }),
                    };
                    let formatted = format!("{message}");
                    const EXPECTED: &str = "ErrorWithNestedError: ErrorWithDeepCompositeShape: \
                        WrappedErrorMessage {some_value=Some(123), \
                        contained=Some($expectedOutput)}";
                    assert_eq!(formatted, EXPECTED);
                    """,
                )
            }
        }

        project.compileAndTest()
    }
}
