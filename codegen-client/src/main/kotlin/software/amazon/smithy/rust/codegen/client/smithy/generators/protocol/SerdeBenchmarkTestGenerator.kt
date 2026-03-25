/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpRequestTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.ClientInstantiator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.BrokenTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.FailingTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.TestCase
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import java.util.logging.Logger
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType as RT

/**
 * Generates serde benchmark loops for protocol tests tagged with `serde-benchmark`.
 *
 * Instead of asserting correctness, each test case becomes a tight loop that
 * measures serialization or deserialization time and prints JSON stats.
 *
 * This generator is specific to protocol test models that use the `serde-benchmark` tag,
 * which is only used internally. It's better to keep the general-purpose [ProtocolTestGenerator]
 * intact rather than bifurcating it to support this special-purpose tag.
 */
class SerdeBenchmarkTestGenerator(
    override val codegenContext: ClientCodegenContext,
    override val protocolSupport: ProtocolSupport,
    override val operationShape: OperationShape,
) : ProtocolTestGenerator() {
    override val appliesTo: AppliesTo = AppliesTo.CLIENT
    override val logger: Logger = Logger.getLogger(javaClass.name)
    override val expectFail: Set<FailingTest> = emptySet()
    override val brokenTests: Set<BrokenTest> = emptySet()
    override val generateOnly: Set<String> = emptySet()
    override val disabledTests: Set<String> = emptySet()

    private val rc = codegenContext.runtimeConfig
    private val inputShape = operationShape.inputShape(codegenContext.model)
    private val instantiator = ClientInstantiator(codegenContext, withinTest = true)

    private val defaultBodyMediaType: String =
        if (codegenContext.protocol.toString() == "smithy.protocols#rpcv2Cbor") "application/cbor" else "unknown"

    override fun RustWriter.renderAllTestCases(allTests: List<TestCase>) {
        for (testCase in allTests) {
            renderBenchmarkTestCaseBlock(testCase, this) {
                when (testCase) {
                    is TestCase.RequestTest -> renderRequestBenchmark(testCase.testCase)
                    is TestCase.ResponseTest -> renderResponseBenchmark(testCase.testCase)
                    is TestCase.MalformedRequestTest -> {}
                }
            }
        }
    }

    private fun renderBenchmarkTestCaseBlock(
        testCase: TestCase,
        writer: RustWriter,
        block: Writable,
    ) {
        if (testCase.documentation != null) {
            writer.docs(testCase.documentation!!, templating = false)
        }
        writer.docs("Benchmark: ${testCase.id}")
        Attribute.TokioTest.render(writer)
        val fnNameSuffix =
            when (testCase) {
                is TestCase.ResponseTest -> "_response"
                is TestCase.RequestTest -> "_request"
                is TestCase.MalformedRequestTest -> "_malformed_request"
            }
        writer.rustBlock("async fn ${testCase.id.toSnakeCase()}$fnNameSuffix()") {
            block(this)
        }
    }

    private fun RustWriter.renderRequestBenchmark(testCase: HttpRequestTestCase) {
        writeInline("let input = ")
        instantiator.render(this, inputShape, testCase.params)
        rust(";")
        rustTemplate(
            """
            use #{RuntimePlugin};
            use #{SerializeRequest};

            let op = #{Operation}::new();
            let config = op.config().expect("operation should have config");
            let serializer = config
                .load::<#{SharedRequestSerializer}>()
                .expect("operation should set a serializer");

            let mut timings = Vec::new();
            for _ in 0..10000 {
                let mut config_bag = #{ConfigBag}::base();
                let input = #{Input}::erase(input.clone());
                let start = std::time::Instant::now();
                let _ = serializer.serialize_input(input, &mut config_bag);
                timings.push(start.elapsed().as_nanos() as u64);
            }
            """,
            "RuntimePlugin" to RT.runtimePlugin(rc),
            "SerializeRequest" to RT.smithyRuntimeApiClient(rc).resolve("client::ser_de::SerializeRequest"),
            "Operation" to codegenContext.symbolProvider.toSymbol(operationShape),
            "SharedRequestSerializer" to RT.smithyRuntimeApiClient(rc).resolve("client::ser_de::SharedRequestSerializer"),
            "ConfigBag" to RT.configBag(rc),
            "Input" to RT.smithyRuntimeApiClient(rc).resolve("client::interceptors::context::Input"),
        )
        renderBenchmarkStats(testCase.id)
    }

    private fun RustWriter.renderResponseBenchmark(testCase: HttpResponseTestCase) {
        val mediaType = testCase.bodyMediaType.orNull()
        rustTemplate(
            """
            use #{DeserializeResponse};
            use #{RuntimePlugin};

            let op = #{Operation}::new();
            let config = op.config().expect("the operation has config");
            let de = config.load::<#{SharedResponseDeserializer}>().expect("the config must have a deserializer");

            let mut timings = Vec::new();
            for _ in 0..10000 {
                let mut http_response = #{Response}::try_from(#{HttpResponseBuilder}::new()
            """,
            "DeserializeResponse" to RT.smithyRuntimeApiClient(rc).resolve("client::ser_de::DeserializeResponse"),
            "RuntimePlugin" to RT.runtimePlugin(rc),
            "Operation" to codegenContext.symbolProvider.toSymbol(operationShape),
            "SharedResponseDeserializer" to
                RT.smithyRuntimeApiClient(rc)
                    .resolve("client::ser_de::SharedResponseDeserializer"),
            "Response" to RT.smithyRuntimeApi(rc).resolve("http::Response"),
            "HttpResponseBuilder" to RT.HttpResponseBuilder1x,
        )
        testCase.headers.forEach { (key, value) ->
            writeWithNoFormatting(".header(${key.dq()}, ${value.dq()})")
        }
        rustTemplate(
            """
            .status(${testCase.code})
            .body(#{SdkBody}::from(${testCase.body.orNull()?.dq()?.replace("#", "##") ?: "vec![]"}))
            .unwrap()
            ).unwrap();
            let start = std::time::Instant::now();
            let parsed = de.deserialize_streaming(&mut http_response);
            let parsed = parsed.unwrap_or_else(|| {
                let http_response = http_response.map(|body| {
                    #{SdkBody}::from(#{copy_from_slice}(&#{decode_body_data}(body.bytes().unwrap(), #{MediaType}::from(${(mediaType ?: defaultBodyMediaType).dq()}))))
                });
                de.deserialize_nonstreaming(&http_response)
            });
            let _ = parsed;
            timings.push(start.elapsed().as_nanos() as u64);
            }
            """,
            "copy_from_slice" to RT.Bytes.resolve("copy_from_slice"),
            "decode_body_data" to RT.protocolTest(rc, "decode_body_data"),
            "MediaType" to RT.protocolTest(rc, "MediaType"),
            "SdkBody" to RT.sdkBody(rc),
        )
        renderBenchmarkStats(testCase.id)
    }

    private fun RustWriter.renderBenchmarkStats(testId: String) {
        rustTemplate(
            """
            let mut sorted = timings.clone();
            sorted.sort_unstable();
            let n = timings.len();
            let mean = timings.iter().sum::<u64>() / n as u64;
            let variance = timings.iter().map(|&x| {
                let diff = x as i64 - mean as i64;
                (diff * diff) as u64
            }).sum::<u64>() / n as u64;

            let result = #{serde_json}::json!({
                "id": "$testId",
                "n": n,
                "mean": mean,
                "p50": sorted[n * 50 / 100],
                "p90": sorted[n * 90 / 100],
                "p95": sorted[n * 95 / 100],
                "p99": sorted[n * 99 / 100],
                "std_dev": (variance as f64).sqrt() as u64
            });
            println!("{}", #{serde_json}::to_string_pretty(&result).unwrap());
            """,
            "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
        )
    }
}
