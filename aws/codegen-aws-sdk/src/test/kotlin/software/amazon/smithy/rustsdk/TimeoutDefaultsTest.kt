/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.assertions.withClue
import io.kotest.matchers.collections.shouldNotContain
import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import java.time.Duration
import kotlin.io.path.Path
import kotlin.io.path.readText

/**
 * Resolution of the per-service default read timeout from the shared timeout-defaults artifact.
 *
 * The three outcomes under test are the three buckets in section 2.3 of the design: the general
 * 5-minute default, the 15-minute partial tier, and full exemption (no timeout at all). Plus the
 * four entries Rust deliberately overrides back to the general default.
 */
class TimeoutDefaultsTest {
    private val artifactPath = Path("../sdk/aws-models/sdk-timeout-defaults.json")

    private fun artifact() = Node.parse(artifactPath.readText()).expectObjectNode()

    private fun load() = TimeoutDefaults(artifact())

    private val fiveMinutes = Duration.ofMinutes(5)
    private val fifteenMinutes = Duration.ofMinutes(15)

    @Test
    fun `a service absent from the artifact gets the general default`() {
        val defaults = load()
        // DynamoDB and STS are deliberately not in the artifact.
        defaults.readTimeoutFor("DynamoDB") shouldBe fiveMinutes
        defaults.readTimeoutFor("STS") shouldBe fiveMinutes
    }

    @Test
    fun `a partially exempt service gets the fifteen minute tier`() {
        val defaults = load()
        defaults.readTimeoutFor("SQS") shouldBe fifteenMinutes
        defaults.readTimeoutFor("EC2") shouldBe fifteenMinutes
        defaults.readTimeoutFor("Kinesis") shouldBe fifteenMinutes
    }

    @Test
    fun `a fully exempt service gets no timeout`() {
        val defaults = load()
        defaults.readTimeoutFor("S3") shouldBe null
        defaults.readTimeoutFor("Glacier") shouldBe null
        defaults.readTimeoutFor("Lambda") shouldBe null
        defaults.readTimeoutFor("Bedrock Runtime") shouldBe null
    }

    @Test
    fun `the Rust overrides take the general default despite being exempt in the artifact`() {
        val defaults = load()
        TimeoutDefaults.RUST_GENERAL_DEFAULT_OVERRIDES.size shouldBe 3
        for (service in TimeoutDefaults.RUST_GENERAL_DEFAULT_OVERRIDES) {
            withClue("$service is overridden back to the general default; see design section 2.3") {
                defaults.readTimeoutFor(service) shouldBe fiveMinutes
            }
        }
    }

    /**
     * Guards against a typo in the override set silently doing nothing.
     *
     * Each override names a key the artifact marks fully exempt. If the name drifts — a trailing
     * space, a case change, a rename upstream — the override stops applying and the service
     * silently keeps its exemption, with no other signal.
     *
     * Note this only proves the *key* exists. Whether any real service carries that `sdkId` is a
     * separate question; see `the SageMaker Runtime HTTP2 artifact key matches no known service`.
     */
    @Test
    fun `every Rust override names an artifact key the artifact actually exempts`() {
        val services = artifact().expectObjectMember("services")
        for (service in TimeoutDefaults.RUST_GENERAL_DEFAULT_OVERRIDES) {
            val entry =
                services.getMember(service).orElseThrow {
                    AssertionError("override '$service' is not a key in the artifact")
                }
            withClue(
                "override '$service' should be fully exempt (-1) in the artifact, otherwise " +
                    "overriding it to the general default is not the change the design describes",
            ) {
                entry.expectNumberNode().value.toLong() shouldBe -1L
            }
        }
    }

    /**
     * A drifted key must fail the build rather than silently resolve to the general default.
     *
     * This is the Rust equivalent of Java's build-time `validateArtifactKeys`, adapted to the fact
     * that smithy-rs vendors a subset of service models. Java generates every service, so it can
     * assert "every key matches a known sdkId". Here that would reject ~59 of 83 keys whose models
     * simply are not in this repo, and it would also reject legitimately forward-looking keys.
     *
     * So the check runs the other way: for each service actually being generated, if its exact
     * `sdkId` is not a key but a key differs only by case or punctuation, that key was meant to
     * cover this service and no longer does. That is unambiguous drift, and it is detectable with
     * any subset of models.
     */
    @Test
    fun `a key that differs from a real sdkId only by punctuation is rejected`() {
        val drifted =
            Node.parse(
                """{ "generalDefaultMillis": 300000, "services": { "SomeService": -1 } }""",
            ).expectObjectNode()
        val defaults = TimeoutDefaults(drifted)
        val e = shouldThrow<RuntimeException> { defaults.readTimeoutFor("Some Service") }
        e.message!! shouldContain "SomeService"
        e.message!! shouldContain "Some Service"
    }

    @Test
    fun `a service genuinely absent from the artifact still falls through to the general default`() {
        val defaults =
            TimeoutDefaults(
                Node.parse(
                    """{ "generalDefaultMillis": 300000, "services": { "SomeService": -1 } }""",
                ).expectObjectNode(),
            )
        defaults.readTimeoutFor("Totally Unrelated") shouldBe fiveMinutes
    }

    @Test
    fun `two keys that normalize alike are rejected when the artifact loads`() {
        val ambiguous =
            Node.parse(
                """{ "generalDefaultMillis": 300000, "services": { "Foo Bar": -1, "FooBar": 900000 } }""",
            ).expectObjectNode()
        shouldThrow<RuntimeException> { TimeoutDefaults(ambiguous) }
    }

    @Test
    fun `the real artifact has no drifted or ambiguous keys for any vendored service`() {
        val defaults = load()
        // Every service model vendored in this repo must resolve without tripping the drift check.
        for (sdkId in vendoredSdkIds()) {
            withClue("resolving $sdkId must not trip the drift check") {
                defaults.readTimeoutFor(sdkId)
            }
        }
    }

    /**
     * `SageMaker Runtime` and `SageMaker Runtime HTTP2` are two different services, and the
     * artifact listing both at different tiers is correct rather than a defect.
     *
     * They have separate models — `sagemaker-runtime` and `sagemaker-runtime-http2` — separate
     * `sdkId`s, and disjoint operations. AWS split the bidirectional streaming operation into its
     * own service because the protocol binding differs. So:
     *
     * - `SageMaker Runtime HTTP2`: one operation, `InvokeEndpointWithBidirectionalStream`, a
     *   bidirectional event stream. Fully exempt in the artifact, overridden by Rust for the same
     *   reason as Transcribe Streaming and ConnectHealth.
     * - `SageMaker Runtime`: `InvokeEndpoint`, `InvokeEndpointAsync`,
     *   `InvokeEndpointWithResponseStream`. Its longest case is a 60-second model invocation, and
     *   its event stream is output-only, which the guidance's own criteria exclude from full
     *   exemption. Hence the 15-minute tier.
     *
     * An earlier revision of this test asserted the HTTP2 key matched no service and called it a
     * dead entry. That was wrong: the model exists, smithy-rs just does not vendor it.
     */
    @Test
    fun `SageMaker Runtime and SageMaker Runtime HTTP2 are distinct services at distinct tiers`() {
        val services = artifact().expectObjectMember("services")
        services.expectNumberMember("SageMaker Runtime HTTP2").value.toLong() shouldBe -1L
        services.expectNumberMember("SageMaker Runtime").value.toLong() shouldBe 900_000L

        val defaults = load()
        // The HTTP/1.1 service keeps its own tier.
        defaults.readTimeoutFor("SageMaker Runtime") shouldBe fifteenMinutes
        // The HTTP/2 service keeps the artifact's exemption. It has the same bidirectional shape as
        // the overridden services, and is deliberately not overridden: its server is a
        // customer-deployed model container, so prompt headers are not knowable. See design 2.3.
        defaults.readTimeoutFor("SageMaker Runtime HTTP2") shouldBe null
        // Neither is overridden.
        TimeoutDefaults.RUST_GENERAL_DEFAULT_OVERRIDES shouldNotContain "SageMaker Runtime"
        TimeoutDefaults.RUST_GENERAL_DEFAULT_OVERRIDES shouldNotContain "SageMaker Runtime HTTP2"
    }

    /**
     * `sdkId` of every service model vendored in this repo.
     *
     * Only `aws/sdk/aws-models/` is consulted, which is a subset — so an absent `sdkId` here means
     * "not vendored", never "no such service". Conflating those is what produced a since-retracted
     * claim that `SageMaker Runtime HTTP2` was not a real service.
     */
    private fun vendoredSdkIds(): List<String> =
        Path("../sdk/aws-models").toFile().listFiles { f: java.io.File ->
            f.name.endsWith(".json") && !f.name.startsWith("sdk-")
        }!!.mapNotNull { f ->
            val shapes = Node.parse(f.readText()).expectObjectNode().expectObjectMember("shapes")
            shapes.members.values.firstNotNullOfOrNull { shape ->
                shape.asObjectNode().orElse(null)
                    ?.takeIf { it.getStringMember("type").orElse(null)?.value == "service" }
                    ?.getObjectMember("traits")?.orElse(null)
                    ?.getObjectMember("aws.api#service")?.orElse(null)
                    ?.getStringMember("sdkId")?.orElse(null)?.value
            }
        }

    @Test
    fun `the artifact has the shape the design audited`() {
        val artifact = artifact()
        artifact.expectNumberMember("generalDefaultMillis").value.toInt() shouldBe 300_000
        val services = artifact.expectObjectMember("services")
        services.members.size shouldBe 83
        val values = services.members.values.map { it.expectNumberNode().value.toLong() }
        values.toSet() shouldBe setOf(-1L, 900_000L)
        values.count { it == -1L } shouldBe 16
        values.count { it == 900_000L } shouldBe 67
    }

    @Test
    fun `an unusable value is rejected rather than silently skipped`() {
        val bad =
            Node.parse(
                """{ "generalDefaultMillis": 300000, "services": { "Thing": 0 } }""",
            ).expectObjectNode()
        shouldThrow<IllegalArgumentException> { TimeoutDefaults(bad) }
    }

    @Test
    fun `the artifact path is reachable from codegen settings`() {
        val settings =
            testClientRustSettings(
                customizationConfig =
                    ObjectNode.builder().withMember(
                        "awsSdk",
                        ObjectNode.builder()
                            .withMember("timeoutDefaultsConfigPath", artifactPath.toString())
                            .build(),
                    ).build(),
            )
        val path = SdkSettings.from(settings).timeoutDefaultsConfigPath
        path shouldNotBe null
        // Loading through the settings path must produce the same answers as loading directly,
        // which is what proves the build wiring reaches the real artifact.
        val viaSettings = TimeoutDefaults(Node.parse(path!!.readText()).expectObjectNode())
        viaSettings.readTimeoutFor("S3") shouldBe null
        viaSettings.readTimeoutFor("Polly") shouldBe fiveMinutes
    }

    /**
     * An absent key must yield null rather than throwing.
     *
     * `SdkSettings.defaultsConfigPath` gets this wrong — it is written `?.value.let { Paths.get(it) }`
     * rather than `?.value?.let`, so it calls `Paths.get(null)` and throws when the `awsSdk` object
     * is absent. It is only safe today because nothing reads it. This test pins the correct form for
     * the key this design adds.
     */
    @Test
    fun `an absent artifact path yields null rather than throwing`() {
        SdkSettings.from(testClientRustSettings()).timeoutDefaultsConfigPath shouldBe null
    }

    /**
     * Lookup is exact, and a near miss is loud.
     *
     * An earlier version of this test asserted that `"s3"` falls through to the general default,
     * which is what the resolver did before the drift check existed. Falling through is the wrong
     * behavior: it is indistinguishable from a renamed service silently losing its exemption, which
     * for S3 would mean quietly applying a 5-minute deadline to `PutObject`. The resolver now
     * refuses rather than guessing in either direction.
     */
    @Test
    fun `lookup is exact, and a near miss fails rather than falling through`() {
        val defaults = load()
        defaults.readTimeoutFor("S3") shouldBe null
        val e = shouldThrow<RuntimeException> { defaults.readTimeoutFor("s3") }
        e.message!! shouldContain "only by case or punctuation"
    }
}
