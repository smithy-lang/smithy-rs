package software.amazon.smithy.rust.codegen.core.smithy.generators.protocol

import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpMalformedRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpMalformedRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.allow
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.testDependenciesOnly
import software.amazon.smithy.rust.codegen.core.util.PANIC
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

/**
 * Common interface to generate protocol tests for a given [operationShape].
 */
abstract class ProtocolTestGenerator {
    abstract val codegenContext: CodegenContext
    abstract val protocolSupport: ProtocolSupport
    abstract val operationShape: OperationShape
    abstract val appliesTo: AppliesTo

    /**
     * We expect these tests to fail due to shortcomings in our implementation.
     * They will _fail_ if they pass, so we will discover and remove them if we fix them by accident.
     **/
    abstract val expectFail: Set<FailingTest>

    /** Only generate these tests; useful to temporarily set and shorten development cycles */
    abstract val runOnly: Set<String>

    /**
     * These tests are not even attempted to be generated, either because they will not compile
     * or because they are flaky.
     */
    abstract val disabledTests: Set<String>

    /** The Rust module in which we should generate the protocol tests for [operationShape]. */
    private fun protocolTestsModule(): RustModule.LeafModule {
        val operationName = codegenContext.symbolProvider.toSymbol(operationShape).name
        val testModuleName = "${operationName.toSnakeCase()}_test"
        val additionalAttributes =
            listOf(Attribute(allow("unreachable_code", "unused_variables")))
        return RustModule.inlineTests(testModuleName, additionalAttributes = additionalAttributes)
    }

    /** The entry point to render the protocol tests, invoked by the code generators. */
    fun render(writer: RustWriter) {
        val allTests = allTestCases().fixBroken()
        if (allTests.isEmpty()) {
            return
        }

        writer.withInlineModule(protocolTestsModule(), null) {
            renderAllTestCases(allTests)
        }
    }

    /** Implementors should describe how to render the test cases. **/
    abstract fun RustWriter.renderAllTestCases(allTests: List<TestCase>)

    /**
     * This function applies a "fix function" to each broken test before we synthesize it.
     * Broken tests are those whose definitions in the `awslabs/smithy` repository are wrong.
     * We try to contribute fixes upstream to pare down this function to the identity function.
     */
    open fun List<TestCase>.fixBroken(): List<TestCase> = this

    /** Filter out test cases that are disabled or don't match the service protocol. */
    private fun List<TestCase>.filterMatching(): List<TestCase> = if (runOnly.isEmpty()) {
        this.filter { testCase -> testCase.protocol == codegenContext.protocol && !disabledTests.contains(testCase.id) }
    } else {
        this.filter { testCase -> runOnly.contains(testCase.id) }
    }

    /** Do we expect this [testCase] to fail? */
    private fun expectFail(testCase: TestCase): Boolean =
        expectFail.find {
            it.id == testCase.id && it.kind == testCase.kind && it.service == codegenContext.serviceShape.id.toString()
        } != null

    fun requestTestCases(): List<TestCase> {
        val requestTests = operationShape.getTrait<HttpRequestTestsTrait>()?.getTestCasesFor(appliesTo).orEmpty()
            .map { TestCase.RequestTest(it) }
        return requestTests.filterMatching()
    }

    fun responseTestCases(): List<TestCase> {
        val operationIndex = OperationIndex.of(codegenContext.model)
        val outputShape = operationShape.outputShape(codegenContext.model)

        // `@httpResponseTests` trait can apply to operation shapes and structure shapes with the `@error` trait.
        // Find both kinds for the operation for which we're generating protocol tests.
        val responseTestsOnOperations =
            operationShape.getTrait<HttpResponseTestsTrait>()
                ?.getTestCasesFor(appliesTo).orEmpty().map { TestCase.ResponseTest(it, outputShape) }
        val responseTestsOnErrors =
            operationIndex.getErrors(operationShape).flatMap { error ->
                val testCases =
                    error.getTrait<HttpResponseTestsTrait>()
                        ?.getTestCasesFor(appliesTo).orEmpty()
                testCases.map { TestCase.ResponseTest(it, error) }
            }

        return (responseTestsOnOperations + responseTestsOnErrors).filterMatching()
    }

    fun malformedRequestTestCases(): List<TestCase> {
        // `@httpMalformedRequestTests` only make sense for servers.
        val malformedRequestTests = if (appliesTo == AppliesTo.SERVER) {
            operationShape.getTrait<HttpMalformedRequestTestsTrait>()
                ?.testCases.orEmpty().map { TestCase.MalformedRequestTest(it) }
        } else {
            emptyList()
        }
        return malformedRequestTests.filterMatching()
    }

    /**
     * Parses from the model and returns all test cases for [operationShape] applying to the [appliesTo] artifact type
     * that should be rendered by implementors.
     **/
    fun allTestCases(): List<TestCase> =
        // Note there's no `@httpMalformedResponseTests`: https://github.com/smithy-lang/smithy/issues/2334
        requestTestCases() + responseTestCases() + malformedRequestTestCases()

    fun renderTestCaseBlock(
        testCase: TestCase,
        testModuleWriter: RustWriter,
        block: Writable,
    ) {
        if (testCase.documentation != null) {
            testModuleWriter.docs(testCase.documentation!!, templating = false)
        }
        testModuleWriter.docs("Test ID: ${testCase.id}")

        // The `#[traced_test]` macro desugars to using `tracing`, so we need to depend on the latter explicitly in
        // case the code rendered by the test does not make use of `tracing` at all.
        val tracingDevDependency = testDependenciesOnly { addDependency(CargoDependency.Tracing.toDevDependency()) }
        testModuleWriter.rustTemplate("#{TracingDevDependency:W}", "TracingDevDependency" to tracingDevDependency)
        Attribute.TokioTest.render(testModuleWriter)
        Attribute.TracedTest.render(testModuleWriter)

        if (expectFail(testCase)) {
            testModuleWriter.writeWithNoFormatting("#[should_panic]")
        }
        val fnNameSuffix = testCase.kind.toString().toSnakeCase()
        testModuleWriter.rustBlock("async fn ${testCase.id.toSnakeCase()}_$fnNameSuffix()") {
            block(this)
        }
    }

    fun checkRequiredHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        requireHeaders: List<String>,
    ) {
        basicCheck(
            requireHeaders,
            rustWriter,
            "required_headers",
            actualExpression,
            "require_headers",
        )
    }

    fun checkForbidHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        forbidHeaders: List<String>,
    ) {
        basicCheck(
            forbidHeaders,
            rustWriter,
            "forbidden_headers",
            actualExpression,
            "forbid_headers",
        )
    }

    fun checkHeaders(
        rustWriter: RustWriter,
        actualExpression: String,
        headers: Map<String, String>,
    ) {
        if (headers.isEmpty()) {
            return
        }
        val variableName = "expected_headers"
        rustWriter.withBlock("let $variableName = [", "];") {
            writeWithNoFormatting(
                headers.entries.joinToString(",") {
                    "(${it.key.dq()}, ${it.value.dq()})"
                },
            )
        }
        assertOk(rustWriter) {
            write(
                "#T($actualExpression, $variableName)",
                RuntimeType.protocolTest(codegenContext.runtimeConfig, "validate_headers"),
            )
        }
    }

    fun basicCheck(
        params: List<String>,
        rustWriter: RustWriter,
        expectedVariableName: String,
        actualExpression: String,
        checkFunction: String,
    ) {
        if (params.isEmpty()) {
            return
        }
        rustWriter.withBlock("let $expectedVariableName = ", ";") {
            strSlice(this, params)
        }
        assertOk(rustWriter) {
            rustWriter.rust(
                "#T($actualExpression, $expectedVariableName)",
                RuntimeType.protocolTest(codegenContext.runtimeConfig, checkFunction),
            )
        }
    }

    /**
     * Wraps `inner` in a call to `aws_smithy_protocol_test::assert_ok`, a convenience wrapper
     * for pretty printing protocol test helper results.
     */
    fun assertOk(
        rustWriter: RustWriter,
        inner: Writable,
    ) {
        rustWriter.rust("#T(", RuntimeType.protocolTest(codegenContext.runtimeConfig, "assert_ok"))
        inner(rustWriter)
        rustWriter.write(");")
    }

    private fun strSlice(
        writer: RustWriter,
        args: List<String>,
    ) {
        writer.withBlock("&[", "]") {
            rust(args.joinToString(",") { it.dq() })
        }
    }
}

/**
 * Service shape IDs in common protocol test suites defined upstream.
 */
object ServiceShapeId {
    const val AWS_JSON_10 = "aws.protocoltests.json10#JsonRpc10"
    const val AWS_JSON_11 = "aws.protocoltests.json#JsonProtocol"
    const val REST_JSON = "aws.protocoltests.restjson#RestJson"
    const val RPC_V2_CBOR = "smithy.protocoltests.rpcv2Cbor#RpcV2Protocol"
    const val RPC_V2_CBOR_EXTRAS = "smithy.protocoltests.rpcv2Cbor#RpcV2Service"
    const val REST_XML = "aws.protocoltests.restxml#RestXml"
    const val AWS_QUERY = "aws.protocoltests.query#AwsQuery"
    const val EC2_QUERY = "aws.protocoltests.ec2#AwsEc2"
    const val REST_JSON_VALIDATION = "aws.protocoltests.restjson.validation#RestJsonValidation"
}

data class FailingTest(val service: String, val id: String, val kind: TestCaseKind)

sealed class TestCaseKind {
    data object Request : TestCaseKind()
    data object Response : TestCaseKind()
    data object MalformedRequest : TestCaseKind()
}

sealed class TestCase {
    data class RequestTest(val testCase: HttpRequestTestCase) : TestCase()
    data class ResponseTest(val testCase: HttpResponseTestCase, val targetShape: StructureShape) : TestCase()
    data class MalformedRequestTest(val testCase: HttpMalformedRequestTestCase) : TestCase()

    /*
     * `HttpRequestTestCase` and `HttpResponseTestCase` both implement `HttpMessageTestCase`, but
     * `HttpMalformedRequestTestCase` doesn't, so we have to define the following trivial delegators to provide a nice
     *  common accessor API.
     */

    val id: String
        get() = when (this) {
            is RequestTest -> this.testCase.id
            is MalformedRequestTest -> this.testCase.id
            is ResponseTest -> this.testCase.id
        }

    val protocol: ShapeId
        get() = when (this) {
            is RequestTest -> this.testCase.protocol
            is MalformedRequestTest -> this.testCase.protocol
            is ResponseTest -> this.testCase.protocol
        }

    val kind: TestCaseKind
        get() = when (this) {
            is RequestTest -> TestCaseKind.Request
            is ResponseTest -> TestCaseKind.Response
            is MalformedRequestTest -> TestCaseKind.MalformedRequest
        }

    val documentation: String?
        get() = when (this) {
            is RequestTest -> this.testCase.documentation.orNull()
            is ResponseTest -> this.testCase.documentation.orNull()
            is MalformedRequestTest -> this.testCase.documentation.orNull()
        }
}
