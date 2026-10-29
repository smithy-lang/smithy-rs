/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.generated.BuildEnvironment
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.util.concurrent.TimeUnit

/**
 * Regular JUnit codegen test, discovered by `codegen-server:test` in CI.
 * Uses the standard generated Rust Tokio tests with real Hyper clients and TCP listeners.
 * Compiles the three test binaries first. Each TCP scenario has its own watchdog;
 * an independent process watchdog also covers test-runtime startup and teardown.
 */
internal class EventStreamTimeoutWireTest {
    @Test
    fun `all event stream deadlines work over TCP`() {
        val artifacts = Path.of(BuildEnvironment.PROJECT_DIR, "codegen-server", "build", "event-stream-wire")
        Files.createDirectories(artifacts)
        // Generation and compilation are preparation, outside the TCP test's deadline.
        for ((protocol, wireProtocol) in listOf(
            "aws.protocols#restJson1" to "rest",
            "aws.protocols#awsJson1_1" to "json",
            "smithy.protocols#rpcv2Cbor" to "cbor",
        )) {
            prepareServer(protocol, wireProtocol, artifacts)
        }
        runRustTests(artifacts)
    }

    private fun runRustTests(artifacts: Path) {
        val started = System.nanoTime()
        val processes = mutableListOf<Triple<String, Process, Long>>()
        try {
            for (protocol in listOf("rest", "json", "cbor")) {
                val log = artifacts.resolve("$protocol-results.txt")
                val process =
                    ProcessBuilder(
                        artifacts.resolve("${protocol}_tests").toString(),
                        "event_stream_timeouts_over_tcp",
                        "--nocapture",
                    ).redirectErrorStream(true).redirectOutput(log.toFile()).start()
                processes.add(Triple(protocol, process, System.nanoTime() + TimeUnit.SECONDS.toNanos(300)))
            }
            for ((protocol, process, deadline) in processes) {
                val remaining = deadline - System.nanoTime()
                check(!process.isAlive || (remaining > 0 && process.waitFor(remaining, TimeUnit.NANOSECONDS))) {
                    "$protocol Rust TCP tests exceeded their five-minute process watchdog:\n" +
                        Files.readString(artifacts.resolve("$protocol-results.txt"))
                }
                val output = Files.readString(artifacts.resolve("$protocol-results.txt"))
                check(process.exitValue() == 0) { "$protocol Rust TCP tests failed:\n$output" }
                // libtest may print its test-name prefix on the same line as
                // the first scenario's output when --nocapture is enabled.
                val scenarios = output.lineSequence().count { it.contains("PASS $protocol ") && !it.contains("PASS $protocol all ") }
                val expectedScenarios =
                    when (protocol) {
                        "rest" -> 10
                        "json" -> 13
                        else -> 15
                    }
                check(scenarios == expectedScenarios) { "Missing scenarios for $protocol:\n$output" }
            }
            val elapsed = (System.nanoTime() - started) / 1_000_000_000.0
            val output =
                listOf("rest", "json", "cbor").joinToString("\n") {
                    Files.readString(artifacts.resolve("$it-results.txt"))
                } + "\nPASS all 38 Rust TCP scenarios in ${elapsed}s (15s watchdog per scenario)\n"
            Files.writeString(artifacts.resolve("results.txt"), output)
            println(output)
        } finally {
            processes.forEach { (_, process) ->
                if (process.isAlive) {
                    process.destroyForcibly()
                    process.waitFor(100, TimeUnit.MILLISECONDS)
                }
            }
        }
    }

    private fun prepareServer(
        protocol: String,
        wireProtocol: String,
        artifacts: Path,
    ) {
        val payload =
            if (wireProtocol == "rest") {
                "@httpPayload"
            } else {
                ""
            }
        val initialFields =
            if (wireProtocol == "cbor") {
                "metadata: String"
            } else {
                ""
            }
        val model =
            """
            ${'$'}version: "2.0"
            namespace test
            use $protocol
            @${protocol.substringAfter("#")}
            service WireService { operations: [PublishEvents, RetryEvents, NoIdleEvents, GlobalIdleEvents, NoInitialEvents, Health] }
            @http(uri: "/PublishEvents", method: "POST")
            operation PublishEvents { input: EventsInput, output: EventsOutput }
            @http(uri: "/RetryEvents", method: "POST")
            operation RetryEvents { input: EventsInput, output: EventsOutput }
            @http(uri: "/NoIdleEvents", method: "POST")
            operation NoIdleEvents { input: EventsInput, output: EventsOutput }
            @http(uri: "/GlobalIdleEvents", method: "POST")
            operation GlobalIdleEvents { input: EventsInput, output: EventsOutput }
            @http(uri: "/NoInitialEvents", method: "POST")
            operation NoInitialEvents { input: NoInitialInput, output: EventsOutput }
            structure NoInitialInput { @required $payload events: Events }
            structure EventsInput { @required $payload events: Events, $initialFields }
            structure EventsOutput { @required count: Integer, @required outcome: String }
            @streaming
            union Events { event: Event }
            structure Event { data: String }
            @http(uri: "/Health", method: "POST")
            operation Health { input := {}, output := {} }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    Node.parse(
                        """
                        {
                            "customizationConfig": {
                                "protocols": {
                                    "common": {
                                        "request": {
                                            "body": {
                                                "eventStreamTimeouts": {
                                                    "initialMessage": "1s",
                                                    "messageCompletion": "1s",
                                                    "idle": "5s",
                                                    "perOperation": {
                                                        "test#PublishEvents": {"idle": "1s"},
                                                        "test#RetryEvents": {"idle": "1s"},
                                                        "test#NoIdleEvents": {"idle": 0},
                                                        "test#NoInitialEvents": {"idle": "1s"}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        """,
                    ).expectObjectNode(),
                command = { crate ->
                    // Build ordinary Cargo test binaries; defer execution so all three
                    // protocols can run concurrently with independent watchdogs.
                    val buildLog = crate.resolve("wire-cargo-test.log")
                    try {
                        "cargo test --quiet --lib --no-run --message-format=json".runCommand(
                            crate,
                            timeout = 300,
                            redirect = ProcessBuilder.Redirect.to(buildLog.toFile()),
                        )
                    } catch (error: Exception) {
                        throw IllegalStateException("Cargo test compilation failed:\n${Files.readString(buildLog)}", error)
                    }
                    val output = Files.readString(buildLog)
                    val executable =
                        output.lineSequence().filter { it.startsWith("{") }
                            .map { Node.parse(it).expectObjectNode() }
                            .mapNotNull {
                                it.getMember("executable").filter { it.isStringNode }
                                    .map { it.expectStringNode().value }.orElse(null)
                            }.last()
                    Files.copy(
                        Path.of(executable),
                        artifacts.resolve("${wireProtocol}_tests"),
                        StandardCopyOption.REPLACE_EXISTING,
                        StandardCopyOption.COPY_ATTRIBUTES,
                    )
                },
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, rustCrate ->
            val runtimeConfig = context.runtimeConfig
            val dependencies =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "Tokio" to CargoDependency.Tokio.withFeature("full").toType(),
                    "Bytes" to CargoDependency.Bytes.toType(),
                    "Http" to RuntimeType.http(runtimeConfig),
                    "Body" to CargoDependency.HttpBody1x.toType(),
                    "BodyUtil" to CargoDependency.HttpBodyUtil01x.toDevDependency().toType(),
                    "Hyper" to ServerCargoDependency.hyperDev(runtimeConfig).withFeature("client").withFeature("http1").toType(),
                    "HyperUtil" to
                        CargoDependency("hyper-util", CratesIo("0.1.17"))
                            .withFeature("tokio").toDevDependency().toType(),
                    "SmithyTypes" to RuntimeType.smithyTypes(runtimeConfig),
                    "EventStream" to RuntimeType.smithyEventStream(runtimeConfig),
                    "SmithyHttp" to RuntimeType.smithyHttp(runtimeConfig),
                    "SmithyCbor" to RuntimeType.smithyCbor(runtimeConfig),
                    "SerdeJson" to CargoDependency.SerdeJson.toType(),
                )
            rustCrate.testModule {
                rustTemplate(
                    resource("wire.rs")
                        .replace("#", "##")
                        .replace("tokio::", "#{Tokio}::")
                        .replace("bytes::", "#{Bytes}::")
                        .replace("http_1x::", "#{Http}::")
                        .replace("http_body_1x::", "#{Body}::")
                        .replace("http_body_util::", "#{BodyUtil}::")
                        .replace("hyper::", "#{Hyper}::")
                        .replace("hyper_util::", "#{HyperUtil}::")
                        .replace("aws_smithy_types::", "#{SmithyTypes}::")
                        .replace("aws_smithy_eventstream::", "#{EventStream}::")
                        .replace("aws_smithy_http::", "#{SmithyHttp}::")
                        .replace("aws_smithy_cbor::", "#{SmithyCbor}::")
                        .replace("serde_json::", "#{SerdeJson}::"),
                    *dependencies,
                )
                val rustProtocol =
                    when (wireProtocol) {
                        "rest" -> "Rest"
                        "json" -> "Json"
                        else -> "Cbor"
                    }
                tokioTest("event_stream_timeouts_over_tcp") {
                    rustTemplate(
                        """
                        wire_scenarios(WireProtocol::$rustProtocol).await;
                        """,
                    )
                }
            }
        }
    }

    private fun resource(name: String): String =
        javaClass.getResourceAsStream("/event-stream-wire/$name")!!.bufferedReader().use { it.readText() }
}
