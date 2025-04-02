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

    private val errorWithCompositeShape = model.lookup<StructureShape>("sample#ErrorWithCompositeShape")
    private val simpleError = model.lookup<StructureShape>("sample#SimpleError")
    private val errorWithDeepCompositeShape = model.lookup<StructureShape>("sample#ErrorWithDeepCompositeShape")
    private val composedSensitiveError = model.lookup<StructureShape>("sample#ComposedSensitiveError")
    private val errorWithNestedError = model.lookup<StructureShape>("sample#ErrorWithNestedError")
    private val errorMessage = model.lookup<StructureShape>("sample#ErrorMessage")
    private val wrappedErrorMessage = model.lookup<StructureShape>("sample#WrappedErrorMessage")
    private val sensitiveMessage = model.lookup<StructureShape>("sample#SensitiveMessage")

    private val allStructures =
        arrayOf(
            errorWithCompositeShape,
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
            errorWithCompositeShape,
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
                rustTemplate(
                    """
                        let message = crate::test_model::ErrorMessage {
                            status_code: "200".to_owned(),
                            error_message: "this is an error".to_owned(),
                            request_id: None,
                            tool_name : "vscode".to_owned()
                        };
                        let formatted = format!("{message}");
                        assert_eq!(formatted, "ErrorMessage {status_code=200, error_message=this is an error, request_id=None, tool_name=vscode}");
                        """,
                )
            }
            unitTest("optional_field_prints_value") {
                rustTemplate(
                    """
                        let message = crate::test_model::ErrorMessage {
                            status_code: "200".to_owned(),
                            error_message: "this is an error".to_owned(),
                            request_id: Some("1234".to_owned()),
                            tool_name : "vscode".to_owned()
                        };
                        let formatted = format!("{message}");
                        assert_eq!(formatted, "ErrorMessage {status_code=200, error_message=this is an error, request_id=Some(1234), tool_name=vscode}");
                        """,
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
                val redacted = REDACTION.removeSurrounding("\"")
                rustTemplate(
                    """
                    let message = crate::test_error::ErrorWithNestedError {
                        message: Some(crate::test_error::ErrorWithDeepCompositeShape {
                            message: Some(crate::test_model::WrappedErrorMessage {
                                some_value: Some(123),
                                contained: Some(crate::test_model::ErrorMessage {
                                    status_code: "200".to_owned(),
                                    error_message: "this is an error".to_owned(),
                                    request_id: Some("1234".to_owned()),
                                    tool_name: "vscode".to_owned(),
                                }),
                            }),
                        }),
                    };
                    let formatted = format!("{message}");
                    const EXPECTED: &str = "ErrorWithNestedError: ErrorWithDeepCompositeShape: \
                        WrappedErrorMessage {some_value=Some(123), \
                        contained=Some(ErrorMessage {status_code=200, \
                        error_message=this is an error, \
                        request_id=Some(1234), \
                        tool_name=vscode})}";
                    assert_eq!(formatted, EXPECTED);
                    """,
                )
            }
        }

        project.compileAndTest()
    }
}
