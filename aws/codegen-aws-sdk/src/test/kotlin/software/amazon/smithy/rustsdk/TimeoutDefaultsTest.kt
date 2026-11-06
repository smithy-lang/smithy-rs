/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.assertions.withClue
import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
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
    fun `the four Rust overrides take the general default despite being exempt in the artifact`() {
        val defaults = load()
        TimeoutDefaults.RUST_GENERAL_DEFAULT_OVERRIDES.size shouldBe 4
        for (service in TimeoutDefaults.RUST_GENERAL_DEFAULT_OVERRIDES) {
            withClue("$service is overridden back to the general default; see design section 2.3") {
                defaults.readTimeoutFor(service) shouldBe fiveMinutes
            }
        }
    }

    /**
     * Guards against a typo in the override set silently doing nothing.
     *
     * Each override names a service the artifact marks fully exempt. If a name drifts — a trailing
     * space, a case change, a rename upstream — the override stops applying and the service
     * silently keeps its exemption. That is a behavior regression with no other signal. Same
     * hazard Java's build-time `validateArtifactKeys` covers.
     */
    @Test
    fun `every Rust override names a service the artifact actually exempts`() {
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

    @Test
    fun `lookup is exact rather than normalized`() {
        val defaults = load()
        // Guards against a resolver that lowercases or strips spaces and then matches the wrong
        // entry. "s3" is not a key, so it must fall through to the general default.
        defaults.readTimeoutFor("s3") shouldBe fiveMinutes
        defaults.readTimeoutFor("S3") shouldBe null
    }
}
