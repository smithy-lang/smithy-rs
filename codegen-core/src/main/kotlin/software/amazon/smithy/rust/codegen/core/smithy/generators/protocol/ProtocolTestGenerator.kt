/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.protocol

import software.amazon.smithy.model.knowledge.OperationIndex
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
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.shouldPanic
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustInlineTemplate
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
import java.util.logging.Logger

/**
 * Common interface to generate protocol tests for a given [operationShape].
 */
abstract class ProtocolTestGenerator {
    abstract val codegenContext: CodegenContext
    abstract val protocolSupport: ProtocolSupport
    abstract val operationShape: OperationShape
    abstract val appliesTo: AppliesTo
    abstract val logger: Logger

    /**
     * We expect these tests to fail due to shortcomings in our implementation.
     * They will _fail_ if they pass, so we will discover and remove them if we fix them by accident.
     */
    abstract val expectFail: Set<FailingTest>

    /**
     * We expect these tests to fail because their definitions are broken.
     * We map from a failing test to a "hotfix" function that can mutate the test in-memory and return a fixed version of it.
     * The tests will _fail_ if they pass, so we will discover and remove the hotfix if we're updating to a newer
     * version of Smithy where the test was fixed upstream.
     */
    abstract val brokenTests: Set<BrokenTest>

    /** Only generate these tests; useful to temporarily set and shorten development cycles */
    abstract val generateOnly: Set<String>

    /**
     * These tests are not even attempted to be generated, either because they will not compile
     * or because they are flaky.
     */
    abstract val disabledTests: Set<String>

    private val serviceShapeId: ShapeId
        get() = codegenContext.serviceShape.id

    /** The Rust module in which we should generate the protocol tests for [operationShape]. */
    private fun protocolTestsModule(): RustModule.LeafModule {
        val operationName = codegenContext.symbolProvider.toSymbol(operationShape).name
        val testModuleName = "${operationName.toSnakeCase()}_test"
        val additionalAttributes = listOf(Attribute(allow("unreachable_code", "unused_variables")))
        return RustModule.inlineTests(testModuleName, additionalAttributes = additionalAttributes)
    }

    /** The entry point to render the protocol tests, invoked by the code generators. */
    fun render(writer: RustWriter) {
        val allTests =
            allMatchingTestCases().flatMap {
                fixBrokenTestCase(it)
            }
                // Filter afterward in case a fixed broken test is disabled.
                .filterMatching()
        if (allTests.isEmpty()) {
            return
        }

        writer.withInlineModule(protocolTestsModule(), null) {
            renderAllTestCases(allTests)
        }
    }

    /**
     * This function applies a "hotfix function" to a broken test case before we synthesize it.
     * Broken tests are those whose definitions in the `smithy-lang/smithy` repository are wrong.
     * We try to contribute fixes upstream to pare down the list of broken tests.
     * If the test is broken, we synthesize it in two versions: the original broken test with a `#[should_panic]`
     * attribute, so get alerted if the test now passes, and the fixed version, which should pass.
     */
    private fun fixBrokenTestCase(it: TestCase): List<TestCase> =
        if (!it.isBroken()) {
            listOf(it)
        } else {
            logger.info("Fixing ${it.kind} test case ${it.id}")

            assert(it.expectFail())

            val brokenTest = it.findInBroken()!!
            var fixed = brokenTest.fixIt(it)

            val intro = "The hotfix function for broken test case ${it.kind} ${it.id}"
            val moreInfo =
                """This test case was identified to be broken in at least these Smithy versions: [${brokenTest.inAtLeast.joinToString()}].
                |We are tracking things here: [${brokenTest.trackedIn.joinToString()}].
                """.trimMargin()

            // Something must change...
            if (it == fixed) {
                PANIC(
                    """$intro did not make any modifications. It is likely that the test case was 
                    |fixed upstream, and you're now updating the Smithy version; in this case, remove the hotfix 
                    |function, as the test is no longer broken.
                    |$moreInfo
                    """.trimMargin(),
                )
            }

            // ... but the hotfix function is not allowed to change the test case kind...
            if (it.kind != fixed.kind) {
                PANIC(
                    """$intro changed the test case kind. This is not allowed.
                    |$moreInfo
                    """.trimMargin(),
                )
            }

            // ... nor its id.
            if (it.id != fixed.id) {
                PANIC(
                    """$intro changed the test case id. This is not allowed.
                    |$moreInfo
                    """.trimMargin(),
                )
            }

            // The latter is because we're going to generate the fixed version with an identifiable suffix.
            fixed = fixed.suffixIdWith("_hotfixed")

            listOf(it, fixed)
        }

    /** Implementors should describe how to render the test cases. **/
    abstract fun RustWriter.renderAllTestCases(allTests: List<TestCase>)

    /** Filter out test cases that are disabled or don't match the service protocol. */
    private fun List<TestCase>.filterMatching(): List<TestCase> =
        if (generateOnly.isEmpty()) {
            this.filter { testCase -> testCase.protocol == codegenContext.protocol && !disabledTests.contains(testCase.id) }
        } else {
            logger.warning("Generating only specified tests")
            this.filter { testCase -> generateOnly.contains(testCase.id) }
        }

    private fun TestCase.toFailingTest(): FailingTest =
        when (this) {
            is TestCase.MalformedRequestTest -> FailingTest.MalformedRequestTest(serviceShapeId.toString(), this.id)
            is TestCase.RequestTest -> FailingTest.RequestTest(serviceShapeId.toString(), this.id)
            is TestCase.ResponseTest -> FailingTest.ResponseTest(serviceShapeId.toString(), this.id)
        }

    /** Do we expect this test case to fail? */
    private fun TestCase.expectFail(): Boolean = this.isBroken() || expectFail.contains(this.toFailingTest())

    /** Is this test case broken? */
    private fun TestCase.isBroken(): Boolean = this.findInBroken() != null

    private fun TestCase.findInBroken(): BrokenTest? =
        brokenTests.find { brokenTest ->
            (this is TestCase.RequestTest && brokenTest is BrokenTest.RequestTest && this.id == brokenTest.id) ||
                (this is TestCase.ResponseTest && brokenTest is BrokenTest.ResponseTest && this.id == brokenTest.id) ||
                (this is TestCase.MalformedRequestTest && brokenTest is BrokenTest.MalformedRequestTest && this.id == brokenTest.id)
        }

    fun requestTestCases(): List<TestCase> {
        val requestTests =
            operationShape.getTrait<HttpRequestTestsTrait>()?.getTestCasesFor(appliesTo).orEmpty()
                .map { TestCase.RequestTest(it) }
        return requestTests
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
                error.getTrait<HttpResponseTestsTrait>()
                    ?.getTestCasesFor(appliesTo).orEmpty().map { TestCase.ResponseTest(it, error) }
            }

        return (responseTestsOnOperations + responseTestsOnErrors)
    }

    fun malformedRequestTestCases(): List<TestCase> {
        // `@httpMalformedRequestTests` only make sense for servers.
        val malformedRequestTests =
            if (appliesTo == AppliesTo.SERVER) {
                operationShape.getTrait<HttpMalformedRequestTestsTrait>()
                    ?.testCases.orEmpty().map { TestCase.MalformedRequestTest(it) }
            } else {
                emptyList()
            }
        return malformedRequestTests
    }

    /**
     * Parses from the model and returns all test cases for [operationShape] applying to the [appliesTo] artifact type
     * that should be rendered by implementors.
     **/
    fun allMatchingTestCases(): List<TestCase> =
        // Note there's no `@httpMalformedResponseTests`: https://github.com/smithy-lang/smithy/issues/2334
        requestTestCases() + responseTestCases() + malformedRequestTestCases()

    fun renderTestCaseBlock(
        testCase: TestCase,
        testModuleWriter: RustWriter,
        block: Writable,
    ) {
        if (testCase.documentation != null) {
            testModuleWriter.rust("")
            testModuleWriter.docs(testCase.documentation!!, templating = false)
        }
        testModuleWriter.docs("Test ID: ${testCase.id}")

        // The `#[traced_test]` macro desugars to using `tracing`, so we need to depend on the latter explicitly in
        // case the code rendered by the test does not make use of `tracing` at all.
        val tracingDevDependency = testDependenciesOnly { addDependency(CargoDependency.Tracing.toDevDependency()) }
        testModuleWriter.rustInlineTemplate("#{TracingDevDependency:W}", "TracingDevDependency" to tracingDevDependency)
        Attribute.TokioTest.render(testModuleWriter)
        Attribute.TracedTest.render(testModuleWriter)

        if (testCase.expectFail()) {
            shouldPanic().render(testModuleWriter)
        }
        val fnNameSuffix =
            when (testCase) {
                is TestCase.ResponseTest -> "_response"
                is TestCase.RequestTest -> "_request"
                is TestCase.MalformedRequestTest -> "_malformed_request"
            }
        testModuleWriter.rustBlock("async fn ${testCase.id.toSnakeCase()}$fnNameSuffix()") {
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

sealed class BrokenTest(
    open val serviceShapeId: String,
    open val id: String,
    /** A non-exhaustive set of Smithy versions where the test was found to be broken. */
    open val inAtLeast: Set<String>,
    /**
     * GitHub URLs related to the test brokenness, like a GitHub issue in Smithy where we reported the test was broken,
     * or a PR where we fixed it.
     **/
    open val trackedIn: Set<String>,
) {
    data class RequestTest(
        override val serviceShapeId: String,
        override val id: String,
        override val inAtLeast: Set<String>,
        override val trackedIn: Set<String>,
        val howToFixItFn: (TestCase.RequestTest) -> TestCase.RequestTest,
    ) : BrokenTest(serviceShapeId, id, inAtLeast, trackedIn)

    data class ResponseTest(
        override val serviceShapeId: String,
        override val id: String,
        override val inAtLeast: Set<String>,
        override val trackedIn: Set<String>,
        val howToFixItFn: (TestCase.ResponseTest) -> TestCase.ResponseTest,
    ) : BrokenTest(serviceShapeId, id, inAtLeast, trackedIn)

    data class MalformedRequestTest(
        override val serviceShapeId: String,
        override val id: String,
        override val inAtLeast: Set<String>,
        override val trackedIn: Set<String>,
        val howToFixItFn: (TestCase.MalformedRequestTest) -> TestCase.MalformedRequestTest,
    ) : BrokenTest(serviceShapeId, id, inAtLeast, trackedIn)

    fun fixIt(testToFix: TestCase): TestCase {
        check(testToFix.id == this.id)
        return when (this) {
            is MalformedRequestTest -> howToFixItFn(testToFix as TestCase.MalformedRequestTest)
            is RequestTest -> howToFixItFn(testToFix as TestCase.RequestTest)
            is ResponseTest -> howToFixItFn(testToFix as TestCase.ResponseTest)
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
    const val RPC_V2_CBOR_EXTRAS = "smithy.protocoltests.rpcv2Cbor#RpcV2CborService"
    const val REST_XML = "aws.protocoltests.restxml#RestXml"
    const val AWS_QUERY = "aws.protocoltests.query#AwsQuery"
    const val EC2_QUERY = "aws.protocoltests.ec2#AwsEc2"
    const val REST_JSON_VALIDATION = "aws.protocoltests.restjson.validation#RestJsonValidation"
}

sealed class FailingTest(open val serviceShapeId: String, open val id: String) {
    data class RequestTest(override val serviceShapeId: String, override val id: String) :
        FailingTest(serviceShapeId, id)

    data class ResponseTest(override val serviceShapeId: String, override val id: String) :
        FailingTest(serviceShapeId, id)

    data class MalformedRequestTest(override val serviceShapeId: String, override val id: String) :
        FailingTest(serviceShapeId, id)
}

sealed class TestCaseKind {
    data object Request : TestCaseKind()

    data object Response : TestCaseKind()

    data object MalformedRequest : TestCaseKind()
}

sealed class TestCase {
    /*
     * The properties of these data classes don't implement `equals()` usefully in Smithy, so we delegate to `equals()`
     * of their `Node` representations.
     */

    data class RequestTest(val testCase: HttpRequestTestCase) : TestCase() {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is RequestTest) return false
            return testCase.toNode().equals(other.testCase.toNode())
        }

        override fun hashCode(): Int = testCase.hashCode()
    }

    data class ResponseTest(val testCase: HttpResponseTestCase, val targetShape: StructureShape) : TestCase() {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is ResponseTest) return false
            return testCase.toNode().equals(other.testCase.toNode())
        }

        override fun hashCode(): Int = testCase.hashCode()
    }

    data class MalformedRequestTest(val testCase: HttpMalformedRequestTestCase) : TestCase() {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is MalformedRequestTest) return false
            return this.protocol == other.protocol && this.id == other.id && this.documentation == other.documentation &&
                this.testCase.request.toNode()
                    .equals(other.testCase.request.toNode()) &&
                this.testCase.response.toNode()
                    .equals(other.testCase.response.toNode())
        }

        override fun hashCode(): Int = testCase.hashCode()
    }

    fun suffixIdWith(suffix: String): TestCase =
        when (this) {
            is RequestTest -> RequestTest(this.testCase.suffixIdWith(suffix))
            is MalformedRequestTest -> MalformedRequestTest(this.testCase.suffixIdWith(suffix))
            is ResponseTest -> ResponseTest(this.testCase.suffixIdWith(suffix), this.targetShape)
        }

    private fun HttpRequestTestCase.suffixIdWith(suffix: String): HttpRequestTestCase =
        this.toBuilder().id(this.id + suffix).build()

    private fun HttpResponseTestCase.suffixIdWith(suffix: String): HttpResponseTestCase =
        this.toBuilder().id(this.id + suffix).build()

    private fun HttpMalformedRequestTestCase.suffixIdWith(suffix: String): HttpMalformedRequestTestCase =
        this.toBuilder().id(this.id + suffix).build()

    /*
     * `HttpRequestTestCase` and `HttpResponseTestCase` both implement `HttpMessageTestCase`, but
     * `HttpMalformedRequestTestCase` doesn't, so we have to define the following trivial delegators to provide a nice
     *  common accessor API.
     */

    val id: String
        get() =
            when (this) {
                is RequestTest -> this.testCase.id
                is MalformedRequestTest -> this.testCase.id
                is ResponseTest -> this.testCase.id
            }

    val protocol: ShapeId
        get() =
            when (this) {
                is RequestTest -> this.testCase.protocol
                is MalformedRequestTest -> this.testCase.protocol
                is ResponseTest -> this.testCase.protocol
            }

    val kind: TestCaseKind
        get() =
            when (this) {
                is RequestTest -> TestCaseKind.Request
                is ResponseTest -> TestCaseKind.Response
                is MalformedRequestTest -> TestCaseKind.MalformedRequest
            }

    val documentation: String?
        get() =
            when (this) {
                is RequestTest -> this.testCase.documentation.orNull()
                is ResponseTest -> this.testCase.documentation.orNull()
                is MalformedRequestTest -> this.testCase.documentation.orNull()
            }
}
