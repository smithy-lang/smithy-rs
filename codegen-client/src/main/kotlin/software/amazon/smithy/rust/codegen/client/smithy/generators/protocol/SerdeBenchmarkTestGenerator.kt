/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.node.ObjectNode
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
import software.amazon.smithy.rust.codegen.core.rustlang.writable
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
 * A response test whose `vendorParams` carry a `serdeBenchmarkControl` object is a
 * *correctness control* instead: it deserializes the response once, outside any timing
 * loop, and asserts the outcome. `{"expectOk": true}` requires success and
 * `{"expectErrorContaining": "..."}` requires a failure whose full error chain contains
 * the text. An optional `schemaSerdeAlsoContaining` adds text that is required only when
 * the client uses schema serde, whose errors carry the parser's message; the legacy
 * generated parser reports a fixed message, and a control must hold on both paths so it
 * can guard either side of an A/B run. Controls print no stats, so they never appear in benchmark results. They
 * exist because the timing loops discard their results, so a loop cannot notice that a
 * response it is timing now fails, or now succeeds where it must not.
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

    private enum class RestJsonNamespaceMode {
        ConfigBag,
        Explicit,
    }

    override fun RustWriter.renderAllTestCases(allTests: List<TestCase>) {
        for (testCase in allTests) {
            val control = (testCase as? TestCase.ResponseTest)?.let { controlOf(it.testCase) }
            if (testCase is TestCase.ResponseTest && control != null) {
                renderControls(testCase, control)
                continue
            }
            renderBenchmarkTestCaseBlock(testCase, this) {
                when (testCase) {
                    is TestCase.RequestTest -> renderRequestBenchmark(testCase.testCase)
                    is TestCase.ResponseTest ->
                        renderResponseBenchmark(testCase.testCase, testCase.id, RestJsonNamespaceMode.ConfigBag)
                    is TestCase.MalformedRequestTest -> {}
                }
            }
            if (
                testCase is TestCase.ResponseTest &&
                codegenContext.protocol == software.amazon.smithy.aws.traits.protocols.RestJson1Trait.ID
            ) {
                val explicitId = "${testCase.id}_ExplicitNamespace"
                renderBenchmarkTestCaseBlock(testCase, this, "_explicit_namespace") {
                    renderResponseBenchmark(testCase.testCase, explicitId, RestJsonNamespaceMode.Explicit)
                }
            }
        }
    }

    private fun controlOf(testCase: HttpResponseTestCase): ObjectNode? =
        testCase.vendorParams.getObjectMember("serdeBenchmarkControl").orNull()

    /** Renders a control once per RestJson1 namespace mode, so both protocol branches are checked. */
    private fun RustWriter.renderControls(
        testCase: TestCase.ResponseTest,
        control: ObjectNode,
    ) {
        val modes =
            if (codegenContext.protocol == software.amazon.smithy.aws.traits.protocols.RestJson1Trait.ID) {
                listOf(RestJsonNamespaceMode.ConfigBag to "", RestJsonNamespaceMode.Explicit to "_explicit_namespace")
            } else {
                listOf(RestJsonNamespaceMode.ConfigBag to "")
            }
        for ((mode, suffix) in modes) {
            testCase.documentation?.let { docs(it, templating = false) }
            docs("Correctness control: ${testCase.id}$suffix")
            Attribute.TokioTest.render(this)
            rustBlock("async fn ${(testCase.id + suffix).toSnakeCase()}_control()") {
                renderResponseControl(testCase.testCase, testCase.id + suffix, control, mode)
            }
        }
    }

    private fun RustWriter.renderResponseControl(
        testCase: HttpResponseTestCase,
        controlId: String,
        control: ObjectNode,
        namespaceMode: RestJsonNamespaceMode,
    ) {
        val expectOk = control.getBooleanMemberOrDefault("expectOk", false)
        val expectError = control.getStringMember("expectErrorContaining").orNull()?.value
        val schemaSerdeAlso =
            control.getStringMember("schemaSerdeAlsoContaining").orNull()?.value?.takeIf {
                software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist
                    .usesSchemaSerdeExclusively(codegenContext)
            }
        val expected = listOfNotNull(expectError, schemaSerdeAlso)
        check(expectOk != (expectError != null)) {
            "serdeBenchmarkControl on $controlId must set exactly one of expectOk or expectErrorContaining"
        }
        val mediaType = testCase.bodyMediaType.orNull()
        val body = testCase.body.orNull()?.dq()?.replace("#", "##") ?: "\"\""
        rustTemplate(
            """
            use #{DeserializeResponse};
            use #{RuntimePlugin};

            let op = #{Operation}::new();
            let config = op.config().expect("the operation has config");
            let de = config.load::<#{SharedResponseDeserializer}>().expect("the config must have a deserializer");
            let mut cfg = #{ConfigBag}::base();
            cfg.push_shared_layer(config.clone());
            #{inject_protocol_de}
            let response_body = #{decode_body_data}($body.as_bytes(), #{MediaType}::from(${(mediaType ?: defaultBodyMediaType).dq()})).into_owned();
            let mut http_response = #{Response}::try_from(#{HttpResponseBuilder}::new()
            """,
            "DeserializeResponse" to RT.smithyRuntimeApiClient(rc).resolve("client::ser_de::DeserializeResponse"),
            "RuntimePlugin" to RT.runtimePlugin(rc),
            "Operation" to codegenContext.symbolProvider.toSymbol(operationShape),
            "SharedResponseDeserializer" to
                RT.smithyRuntimeApiClient(rc).resolve("client::ser_de::SharedResponseDeserializer"),
            "Response" to RT.smithyRuntimeApi(rc).resolve("http::Response"),
            "HttpResponseBuilder" to RT.HttpResponseBuilder1x,
            "ConfigBag" to RT.smithyTypes(rc).resolve("config_bag::ConfigBag"),
            "decode_body_data" to RT.protocolTest(rc, "decode_body_data"),
            "MediaType" to RT.protocolTest(rc, "MediaType"),
            "inject_protocol_de" to protocolConfigBagSetup("cfg", namespaceMode),
        )
        testCase.headers.forEach { (key, value) ->
            writeWithNoFormatting(".header(${key.dq()}, ${value.dq()})")
        }
        rustTemplate(
            """
            .status(${testCase.code})
            .body(#{SdkBody}::from(response_body))
            .unwrap()
            ).unwrap();
            let parsed = de.deserialize_streaming_with_config(&mut http_response, &cfg);
            let parsed = parsed.unwrap_or_else(|| de.deserialize_nonstreaming_with_config(&http_response, &cfg));
            """,
            "SdkBody" to RT.sdkBody(rc),
        )
        val errorContext = RT.smithyTypes(rc).resolve("error::display::DisplayErrorContext")
        if (expectError != null) {
            rustTemplate(
                """
                match parsed {
                    Ok(_) => panic!("$controlId: expected a failure, but deserialization succeeded"),
                    Err(err) => {
                        let message = format!("{}", #{DisplayErrorContext}(&err));
                        for expected in [${expected.joinToString { it.dq() }}] {
                            assert!(
                                message.contains(expected),
                                "$controlId: expected the error to contain {:?}, got: {}", expected, message
                            );
                        }
                    }
                }
                """,
                "DisplayErrorContext" to errorContext,
            )
        } else {
            rustTemplate(
                """
                if let Err(err) = parsed {
                    panic!("$controlId: expected success, got: {}", #{DisplayErrorContext}(&err));
                }
                """,
                "DisplayErrorContext" to errorContext,
            )
        }
    }

    private fun renderBenchmarkTestCaseBlock(
        testCase: TestCase,
        writer: RustWriter,
        nameSuffix: String = "",
        block: Writable,
    ) {
        if (testCase.documentation != null) {
            writer.docs(testCase.documentation!!, templating = false)
        }
        writer.docs("Benchmark: ${testCase.id}$nameSuffix")
        Attribute.TokioTest.render(writer)
        val fnNameSuffix =
            when (testCase) {
                is TestCase.ResponseTest -> "_response"
                is TestCase.RequestTest -> "_request"
                is TestCase.MalformedRequestTest -> "_malformed_request"
            }
        writer.rustBlock("async fn ${(testCase.id + nameSuffix).toSnakeCase()}$fnNameSuffix()") {
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

            let mut config_bag = #{ConfigBag}::base();
            config_bag.push_shared_layer(config.clone());
            #{inject_protocol_ser}
            let mut timings = Vec::new();
            for _ in 0..10000 {
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
            "inject_protocol_ser" to protocolConfigBagSetup("config_bag"),
        )
        renderBenchmarkStats(testCase.id)
    }

    private fun RustWriter.renderResponseBenchmark(
        testCase: HttpResponseTestCase,
        benchmarkId: String,
        namespaceMode: RestJsonNamespaceMode,
    ) {
        val mediaType = testCase.bodyMediaType.orNull()
        val body = testCase.body.orNull()?.dq()?.replace("#", "##") ?: "\"\""
        rustTemplate(
            """
            use #{DeserializeResponse};
            use #{RuntimePlugin};

            let op = #{Operation}::new();
            let config = op.config().expect("the operation has config");
            let de = config.load::<#{SharedResponseDeserializer}>().expect("the config must have a deserializer");
            let mut cfg = #{ConfigBag}::base();
            cfg.push_shared_layer(config.clone());
            #{inject_protocol_de}
            let response_body = #{copy_from_slice}(
                &#{decode_body_data}($body.as_bytes(), #{MediaType}::from(${(mediaType ?: defaultBodyMediaType).dq()}))
            );

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
            "ConfigBag" to RT.smithyTypes(rc).resolve("config_bag::ConfigBag"),
            "copy_from_slice" to RT.Bytes.resolve("copy_from_slice"),
            "decode_body_data" to RT.protocolTest(rc, "decode_body_data"),
            "MediaType" to RT.protocolTest(rc, "MediaType"),
            "inject_protocol_de" to protocolConfigBagSetup("cfg", namespaceMode),
        )
        testCase.headers.forEach { (key, value) ->
            writeWithNoFormatting(".header(${key.dq()}, ${value.dq()})")
        }
        rustTemplate(
            """
            .status(${testCase.code})
            .body(#{SdkBody}::from(response_body.clone()))
            .unwrap()
            ).unwrap();
            let start = std::time::Instant::now();
            let parsed = de.deserialize_streaming_with_config(&mut http_response, &cfg);
            let parsed = parsed.unwrap_or_else(|| de.deserialize_nonstreaming_with_config(&http_response, &cfg));
            let _ = std::hint::black_box(parsed);
            timings.push(start.elapsed().as_nanos() as u64);
            }
            """,
            "SdkBody" to RT.sdkBody(rc),
        )
        renderBenchmarkStats(benchmarkId)
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

    /** Generates Rust code to inject the protocol into a benchmark config bag. */
    private fun protocolConfigBagSetup(
        cfgVarName: String,
        namespaceMode: RestJsonNamespaceMode = RestJsonNamespaceMode.ConfigBag,
    ): software.amazon.smithy.rust.codegen.core.rustlang.Writable =
        writable {
            if (software.amazon.smithy.rust.codegen.client.smithy.customizations.SchemaSerdeAllowlist.usesSchemaSerdeExclusively(codegenContext)) {
                val smithyJson = CargoDependency.smithyJson(codegenContext.runtimeConfig).toType()
                val smithyXml = CargoDependency.smithyXml(codegenContext.runtimeConfig).toType()
                val smithyCbor = CargoDependency.smithyCbor(codegenContext.runtimeConfig).toType()
                val smithySchema = RT.smithySchema(codegenContext.runtimeConfig)
                val protocol = codegenContext.protocol
                val serviceShapeName = codegenContext.serviceShape.id.name
                val serviceNamespace = codegenContext.serviceShape.id.namespace

                val (protocolType, constructor) =
                    when {
                        protocol == software.amazon.smithy.aws.traits.protocols.RestJson1Trait.ID -> {
                            val constructor =
                                when (namespaceMode) {
                                    RestJsonNamespaceMode.ConfigBag -> "new()"
                                    RestJsonNamespaceMode.Explicit ->
                                        "new().with_default_namespace(${serviceNamespace.dq()})"
                                }
                            smithyJson.resolve("protocol::aws_rest_json_1::AwsRestJsonProtocol") to constructor
                        }
                        protocol == software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait.ID ->
                            smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_0().with_target_prefix(${serviceShapeName.dq()})"
                        protocol == software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait.ID ->
                            smithyJson.resolve("protocol::aws_json_rpc::AwsJsonRpcProtocol") to "aws_json_1_1().with_target_prefix(${serviceShapeName.dq()})"
                        protocol == software.amazon.smithy.aws.traits.protocols.RestXmlTrait.ID ->
                            smithyXml.resolve("protocol::aws_rest_xml::AwsRestXmlProtocol") to "new()"
                        protocol == software.amazon.smithy.aws.traits.protocols.AwsQueryTrait.ID -> {
                            val smithyQuery = CargoDependency.smithyQuery(codegenContext.runtimeConfig).toType()
                            smithyQuery.resolve("protocol::AwsQueryProtocol") to "new().with_service_version(${codegenContext.serviceShape.version.dq()})"
                        }
                        protocol == software.amazon.smithy.protocol.traits.Rpcv2CborTrait.ID ->
                            smithyCbor.resolve("protocol::RpcV2CborProtocol") to "new()"
                        else -> return@writable
                    }

                rustTemplate(
                    """
                    {
                        let mut layer = #{Layer}::new("bench_protocol");
                        layer.store_put(#{SharedClientProtocol}::new(#{ProtocolType}::$constructor));
                        layer.store_put(#{NonUtf8HeaderHandling}::Reject);
                        $cfgVarName.push_shared_layer(layer.freeze());
                    }
                    """,
                    "Layer" to RT.smithyTypes(codegenContext.runtimeConfig).resolve("config_bag::Layer"),
                    "SharedClientProtocol" to smithySchema.resolve("protocol::SharedClientProtocol"),
                    "ProtocolType" to protocolType,
                    "NonUtf8HeaderHandling" to
                        RT.smithyRuntimeApi(codegenContext.runtimeConfig).resolve("http::NonUtf8HeaderHandling"),
                )
            }
        }
}
