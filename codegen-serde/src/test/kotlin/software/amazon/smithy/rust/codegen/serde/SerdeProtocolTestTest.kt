/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.serde

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.node.StringNode
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.DoubleShape
import software.amazon.smithy.model.shapes.FloatShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ClientInstantiator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolTestGenerator
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.BrokenTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.FailingTest
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolSupport
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ServiceShapeId.REST_JSON
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.TestCase
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerInstantiator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolTestGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerRestJsonFactory
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import java.util.logging.Logger

class SerdeProtocolTestTest {
    private val semanticallyLossyRoundTripTests =
        setOf(
            // These values contain Some(Document::Null). Serde encodes both that and None as null.
            "RestJsonServerPopulatesDefaultsWhenMissingInRequestBody",
            "RestJsonServerPopulatesDefaultsInResponseWhenMissingInParams",
        )

    private enum class NonFiniteValues {
        NONE,
        INFINITY,
        NAN,
        ;

        fun combine(other: NonFiniteValues): NonFiniteValues = maxOf(this, other)
    }

    private val behaviorModel =
        """
        namespace com.example
        use smithy.rust#serde
        use aws.protocols#awsJson1_0
        use smithy.framework#ValidationException

        @awsJson1_0
        @serde(serialize: false, deserialize: true)
        service BehaviorService {
            operations: [Check]
        }

        operation Check {
            input: CheckInput
            errors: [ValidationException]
        }

        structure CheckInput {
            value: String
            kind: TestEnum
            choice: Choice
        }

        enum TestEnum {
            A
            B
        }

        union Choice {
            text: String
            empty: Unit
        }
        """.asSmithyModel(smithyVersion = "2")

    private fun Model.attachSerdeToService(serviceShapeId: ShapeId): Model {
        val service =
            this.expectShape(serviceShapeId, ServiceShape::class.java).toBuilder().addTrait(
                SerdeTrait(true, true, null, null, SourceLocation.NONE),
            ).build()
        return ModelTransformer.create().mapShapes(this) { serviceShape ->
            serviceShape.letIf(serviceShape.id == serviceShapeId) {
                service
            }
        }
    }

    private fun restJsonProtocolTestModel(): Model {
        val serviceShapeId = ShapeId.from(REST_JSON)
        return Model.assembler()
            .discoverModels()
            .assemble()
            .result
            .get()
            .attachSerdeToService(serviceShapeId)
    }

    private fun noRenderedProtocolTests(base: ProtocolTestGenerator): ProtocolTestGenerator =
        object : ProtocolTestGenerator() {
            override val codegenContext: CodegenContext
                get() = base.codegenContext
            override val protocolSupport: ProtocolSupport
                get() = base.protocolSupport
            override val operationShape: OperationShape
                get() = base.operationShape
            override val appliesTo: AppliesTo
                get() = base.appliesTo
            override val logger: Logger
                get() = base.logger
            override val expectFail: Set<FailingTest>
                get() = base.expectFail
            override val brokenTests: Set<BrokenTest>
                get() = base.brokenTests
            override val generateOnly: Set<String>
                get() = base.generateOnly
            override val disabledTests: Set<String>
                get() = base.disabledTests

            override fun RustWriter.renderAllTestCases(allTests: List<TestCase>) {}
        }

    private fun nonFiniteValues(
        model: Model,
        shape: Shape,
        value: Node,
    ): NonFiniteValues =
        when (shape) {
            is MemberShape -> nonFiniteValues(model, model.expectShape(shape.target), value)
            is FloatShape, is DoubleShape ->
                when ((value as? StringNode)?.value) {
                    "NaN" -> NonFiniteValues.NAN
                    "Infinity", "-Infinity" -> NonFiniteValues.INFINITY
                    else -> NonFiniteValues.NONE
                }
            is StructureShape, is UnionShape -> {
                val objectValue = value as? ObjectNode ?: return NonFiniteValues.NONE
                objectValue.members.entries.fold(NonFiniteValues.NONE) { result, (name, memberValue) ->
                    val member = shape.getMember(name.value).orElse(null)
                    if (member == null) result else result.combine(nonFiniteValues(model, member, memberValue))
                }
            }
            is CollectionShape -> {
                val arrayValue = value as? ArrayNode ?: return NonFiniteValues.NONE
                arrayValue.elements.fold(NonFiniteValues.NONE) { result, memberValue ->
                    result.combine(nonFiniteValues(model, shape.member, memberValue))
                }
            }
            is MapShape -> {
                val objectValue = value as? ObjectNode ?: return NonFiniteValues.NONE
                objectValue.members.values.fold(NonFiniteValues.NONE) { result, memberValue ->
                    result.combine(nonFiniteValues(model, shape.value, memberValue))
                }
            }
            else -> NonFiniteValues.NONE
        }

    private fun nonFiniteHeaderValues(
        model: Model,
        shape: StructureShape,
        headers: Map<String, String>,
    ): NonFiniteValues =
        shape.members().fold(NonFiniteValues.NONE) { result, member ->
            val headerName = member.getTrait(HttpHeaderTrait::class.java).orElse(null)?.value
            val headerValue = headerName?.let(headers::get)
            if (headerValue == null) {
                result
            } else {
                result.combine(nonFiniteValues(model, member, Node.from(headerValue)))
            }
        }

    private fun RustWriter.renderRoundTripSupport() {
        rustTemplate(
            """
            use crate::serde::{
                DeserializationSettings,
                SerializationSettings,
                SerializeConfigured,
            };
            use #{rstest}::rstest;

            ##[derive(Clone, Copy, Debug)]
            enum RoundTripFormat {
                Json,
                Cbor,
            }

            fn assert_round_trip<T>(
                expected: T,
                format: RoundTripFormat,
                out_of_range_floats_as_strings: bool,
                contains_nan: bool,
            )
            where
                T: SerializeConfigured + #{DeserializeOwned} + #{PartialEq} + #{Debug},
            {
                let mut serialization = SerializationSettings::default();
                serialization.out_of_range_floats_as_strings =
                    out_of_range_floats_as_strings;
                serialization.serialize_unset_fields = true;

                let encoded = match format {
                    RoundTripFormat::Json => {
                        #{serde_json}::to_vec(&expected.serialize_ref(&serialization))
                            .expect("failed to serialize JSON")
                    }
                    RoundTripFormat::Cbor => {
                        let mut encoded = #{Vec}::new();
                        #{ciborium}::ser::into_writer(
                            &expected.serialize_ref(&serialization),
                            &mut encoded,
                        ).expect("failed to serialize CBOR");
                        encoded
                    }
                };

                let mut deserialization = DeserializationSettings::default();
                deserialization.allow_non_finite_float_strings =
                    out_of_range_floats_as_strings;
                let actual: T = deserialization.scope(|| match format {
                    RoundTripFormat::Json => {
                        #{serde_json}::from_slice(&encoded)
                            .expect("failed to deserialize JSON")
                    }
                    RoundTripFormat::Cbor => {
                        #{ciborium}::de::from_reader(encoded.as_slice())
                            .expect("failed to deserialize CBOR")
                    }
                });

                if contains_nan {
                    let expected_representation = #{serde_json}::to_value(
                        &expected.serialize_ref(&serialization),
                    ).expect("failed to capture expected serde representation");
                    let actual_representation = #{serde_json}::to_value(
                        &actual.serialize_ref(&serialization),
                    ).expect("failed to capture actual serde representation");
                    assert_eq!(
                        expected_representation,
                        actual_representation,
                        "serde representation changed after {format:?} round trip \
                         with out_of_range_floats_as_strings={out_of_range_floats_as_strings}",
                    );
                } else {
                    assert_eq!(
                        expected,
                        actual,
                        "semantic value changed after {format:?} round trip \
                         with out_of_range_floats_as_strings={out_of_range_floats_as_strings}",
                    );
                }
            }
            """,
            "rstest" to CargoDependency("rstest", CratesIo("0.23")).toDevDependency().toType(),
            "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
            "ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
            "DeserializeOwned" to CargoDependency.Serde.toDevDependency().toType().resolve("de::DeserializeOwned"),
            "PartialEq" to RuntimeType.std.resolve("cmp::PartialEq"),
            "Debug" to RuntimeType.std.resolve("fmt::Debug"),
            *RuntimeType.preludeScope,
        )
    }

    private fun RustWriter.renderRoundTripTest(
        operationShape: OperationShape,
        testCase: TestCase,
        expected: software.amazon.smithy.rust.codegen.core.rustlang.Writable,
        nonFiniteValues: NonFiniteValues,
    ) {
        val kind =
            when (testCase) {
                is TestCase.RequestTest -> "request"
                is TestCase.ResponseTest -> "response"
                is TestCase.MalformedRequestTest -> error("malformed requests do not contain semantic values")
            }
        val testName =
            listOf("round_trip", operationShape.id.name, testCase.id, kind)
                .joinToString("_")
                .toSnakeCase()
        val containsNan = nonFiniteValues == NonFiniteValues.NAN

        rustTemplate(
            """
            ##[rstest]
            #{DefaultCases:W}
            ##[case::json_non_finite_strings(RoundTripFormat::Json, true)]
            ##[case::cbor_non_finite_strings(RoundTripFormat::Cbor, true)]
            fn $testName(
                ##[case] format: RoundTripFormat,
                ##[case] out_of_range_floats_as_strings: bool,
            ) {
                let expected = #{Expected:W};
                assert_round_trip(
                    expected,
                    format,
                    out_of_range_floats_as_strings,
                    $containsNan,
                );
            }
            """,
            "DefaultCases" to
                software.amazon.smithy.rust.codegen.core.rustlang.writable {
                    if (nonFiniteValues == NonFiniteValues.NONE) {
                        rustTemplate(
                            """
                            ##[case::json_default(RoundTripFormat::Json, false)]
                            ##[case::cbor_default(RoundTripFormat::Cbor, false)]
                            """,
                        )
                    }
                },
            "Expected" to expected,
        )
    }

    private fun renderProtocolRoundTrips(
        codegenContext: CodegenContext,
        rustCrate: RustCrate,
        instantiator: Instantiator,
        protocolTestGenerator: (OperationShape) -> ProtocolTestGenerator,
    ) {
        val model = codegenContext.model
        val operationShapes =
            TopDownIndex.of(model)
                .getContainedOperations(codegenContext.serviceShape)
                .sortedBy { it.id.toString() }

        rustCrate.testModule {
            renderRoundTripSupport()

            for (operationShape in operationShapes) {
                val generator = protocolTestGenerator(operationShape)
                val inputShape = operationShape.inputShape(model)

                for (testCase in generator.requestTestCases().filter { it.protocol == codegenContext.protocol }) {
                    check(testCase is TestCase.RequestTest)
                    if (
                        inputShape.hasStreamingMember(model) ||
                        testCase.id in semanticallyLossyRoundTripTests
                    ) {
                        continue
                    }

                    val values =
                        nonFiniteValues(model, inputShape, testCase.testCase.params)
                            .combine(nonFiniteHeaderValues(model, inputShape, testCase.testCase.headers))

                    renderRoundTripTest(
                        operationShape,
                        testCase,
                        instantiator.generate(
                            inputShape,
                            testCase.testCase.params,
                            testCase.testCase.headers,
                        ),
                        values,
                    )
                }

                for (testCase in generator.responseTestCases().filter { it.protocol == codegenContext.protocol }) {
                    check(testCase is TestCase.ResponseTest)
                    if (
                        testCase.targetShape.hasStreamingMember(model) ||
                        testCase.id in semanticallyLossyRoundTripTests
                    ) {
                        continue
                    }

                    val values = nonFiniteValues(model, testCase.targetShape, testCase.testCase.params)

                    renderRoundTripTest(
                        operationShape,
                        testCase,
                        instantiator.generate(testCase.targetShape, testCase.testCase.params),
                        values,
                    )
                }
            }
        }
    }

    @Test
    fun testClientRestJsonProtocolValuesRoundTripThroughSerde() {
        val noProtocolTestsDecorator =
            object : ClientCodegenDecorator {
                override val name: String = "Suppress HTTP protocol tests for serde round trips"
                override val order: Byte = 0

                override fun protocolTestGenerator(
                    codegenContext: ClientCodegenContext,
                    baseGenerator: ProtocolTestGenerator,
                ): ProtocolTestGenerator = noRenderedProtocolTests(baseGenerator)
            }
        val clientProtocolSupport =
            ProtocolSupport(
                requestSerialization = true,
                requestBodySerialization = true,
                responseDeserialization = true,
                errorDeserialization = true,
                requestDeserialization = false,
                requestBodyDeserialization = false,
                responseSerialization = false,
                errorSerialization = false,
            )

        clientIntegrationTest(
            restJsonProtocolTestModel(),
            IntegrationTestParams(
                service = REST_JSON,
                cargoCommand = "cargo test --quiet --all-features round_trip_ -- --test-threads=1",
            ),
            additionalDecorators = listOf(noProtocolTestsDecorator),
        ) { codegenContext, rustCrate ->
            renderProtocolRoundTrips(
                codegenContext,
                rustCrate,
                ClientInstantiator(codegenContext),
            ) { operationShape ->
                ClientProtocolTestGenerator(codegenContext, clientProtocolSupport, operationShape)
            }
        }
    }

    @Test
    fun testServerRestJsonProtocolValuesRoundTripThroughSerde() {
        val noProtocolTestsDecorator =
            object : ServerCodegenDecorator {
                override val name: String = "Suppress HTTP protocol tests for serde round trips"
                override val order: Byte = 0

                override fun protocolTestGenerator(
                    codegenContext: ServerCodegenContext,
                    baseGenerator: ProtocolTestGenerator,
                ): ProtocolTestGenerator = noRenderedProtocolTests(baseGenerator)
            }

        serverIntegrationTest(
            restJsonProtocolTestModel(),
            IntegrationTestParams(
                service = REST_JSON,
                cargoCommand = "cargo test --quiet --all-features round_trip_ -- --test-threads=1",
            ),
            additionalDecorators = listOf(noProtocolTestsDecorator),
            testCoverage = HttpTestType.Default,
        ) { codegenContext, rustCrate ->
            renderProtocolRoundTrips(
                codegenContext,
                rustCrate,
                ServerInstantiator(codegenContext),
            ) { operationShape ->
                ServerProtocolTestGenerator(
                    codegenContext,
                    ServerRestJsonFactory().support(),
                    operationShape,
                )
            }
        }
    }

    @ParameterizedTest
    @ValueSource(booleans = [true, false])
    fun testConstraintsModel(usePublicConstrainedTypes: Boolean) {
        val constraintsService = ShapeId.from("com.amazonaws.constraints#ConstraintsService")
        val filePath = "../codegen-core/common-test-models/constraints.smithy"
        val model = File(filePath).readText().asSmithyModel().attachSerdeToService(constraintsService)
        val constrainedShapesSettings =
            Node.objectNodeBuilder().withMember(
                "codegen",
                Node.objectNodeBuilder()
                    .withMember("publicConstrainedTypes", usePublicConstrainedTypes)
                    .build(),
            ).build()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = constraintsService.toString(),
                cargoCommand = "cargo test --all-features",
                additionalSettings = constrainedShapesSettings,
            ),
        ) { _, _ ->
        }
    }

    @ParameterizedTest
    @ValueSource(booleans = [true, false])
    fun testDeserializationConstraintValidation(usePublicConstrainedTypes: Boolean) {
        val service = ShapeId.from("com.example#ConstraintService")
        val model =
            """
            namespace com.example
            use aws.protocols#awsJson1_0
            use smithy.framework#ValidationException

            @awsJson1_0
            service ConstraintService {
                operations: [Check]
            }

            operation Check {
                input: CheckInput
                output: CheckOutput
                errors: [ValidationException]
            }

            structure CheckInput {
                @required
                value: BoundedString
            }

            @length(min: 2, max: 4)
            string BoundedString

            structure CheckOutput {
                choice: OutputChoice
            }

            union OutputChoice {
                value: OutputBoundedString
            }

            @length(min: 2, max: 4)
            string OutputBoundedString
            """.asSmithyModel(smithyVersion = "2").attachSerdeToService(service)
        val constrainedShapesSettings =
            Node.objectNodeBuilder().withMember(
                "codegen",
                Node.objectNodeBuilder()
                    .withMember("publicConstrainedTypes", usePublicConstrainedTypes)
                    .build(),
            ).build()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = service.toString(),
                cargoCommand = "cargo test --all-features",
                additionalSettings = constrainedShapesSettings,
            ),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(codegenContext.moduleUseName()),
                    "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                )

            rustCrate.integrationTest("constraint_deserialization") {
                unitTest("constrained_values_are_validated") {
                    rustTemplate(
                        """
                        use #{crate}::input::CheckInput;

                        let valid: Result<CheckInput, _> =
                            #{serde_json}::from_str(r##"{"value":"okay"}"##);
                        assert!(valid.is_ok(), "valid constrained value was rejected: {valid:?}");

                        let invalid: Result<CheckInput, _> =
                            #{serde_json}::from_str(r##"{"value":"x"}"##);
                        assert!(invalid.is_err(), "invalid constrained value was accepted");

                        let missing: Result<CheckInput, _> = #{serde_json}::from_str("{}");
                        assert!(missing.is_err(), "missing required value was accepted");

                        use #{crate}::output::CheckOutput;
                        let valid_output: Result<CheckOutput, _> =
                            #{serde_json}::from_str(r##"{"choice":{"value":"okay"}}"##);
                        assert!(
                            valid_output.is_ok(),
                            "valid constrained union value was rejected: {valid_output:?}"
                        );

                        let invalid_output: Result<CheckOutput, _> =
                            #{serde_json}::from_str(r##"{"choice":{"value":"x"}}"##);
                        assert!(
                            invalid_output.is_err(),
                            "invalid output-only union constraint was accepted"
                        );
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    @ParameterizedTest
    @ValueSource(booleans = [true, false])
    fun testClientDeserializationBehavior(useCbor: Boolean) {
        clientIntegrationTest(
            behaviorModel,
            IntegrationTestParams(
                service = "com.example#BehaviorService",
                cargoCommand = "cargo test --all-features",
            ),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(codegenContext.moduleUseName()),
                    "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                    "ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
                )

            rustCrate.integrationTest("client_deserialization_behavior_$useCbor") {
                unitTest("structure_enum_and_union_behavior") {
                    rustTemplate(
                        """
                        use #{crate}::operation::check::CheckInput;

                        fn decode(input: &str) -> Result<CheckInput, String> {
                            if $useCbor {
                                let value: #{serde_json}::Value =
                                    #{serde_json}::from_str(input).map_err(|err| err.to_string())?;
                                let mut bytes = Vec::new();
                                #{ciborium}::ser::into_writer(&value, &mut bytes)
                                    .map_err(|err| err.to_string())?;
                                #{ciborium}::de::from_reader(bytes.as_slice())
                                    .map_err(|err| err.to_string())
                            } else {
                                #{serde_json}::from_str(input).map_err(|err| err.to_string())
                            }
                        }

                        let value = decode(
                            r##"{"value":"hello","kind":"FUTURE","choice":{"text":"world"},"ignored":{"nested":true}}"##
                        ).expect("unknown fields and client enum variants should be accepted");
                        assert_eq!(value.value.as_deref(), Some("hello"));
                        assert_eq!(value.kind.expect("kind should be set").as_str(), "FUTURE");

                        assert!(decode(r##"{"choice":{"future":null}}"##).is_err());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("duplicate_known_fields_are_rejected") {
                    rustTemplate(
                        """
                        use #{crate}::operation::check::CheckInput;

                        let duplicate = r##"{"value":"first","value":"second"}"##;
                        let result: Result<CheckInput, _> = #{serde_json}::from_str(duplicate);
                        assert!(result.is_err());
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    @ParameterizedTest
    @ValueSource(booleans = [true, false])
    fun testServerDeserializationBehavior(usePublicConstrainedTypes: Boolean) {
        val settings =
            Node.objectNodeBuilder().withMember(
                "codegen",
                Node.objectNodeBuilder()
                    .withMember("publicConstrainedTypes", usePublicConstrainedTypes)
                    .build(),
            ).build()
        serverIntegrationTest(
            behaviorModel,
            IntegrationTestParams(
                service = "com.example#BehaviorService",
                cargoCommand = "cargo test --all-features",
                additionalSettings = settings,
            ),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(codegenContext.moduleUseName()),
                    "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                )

            rustCrate.integrationTest("server_deserialization_behavior") {
                unitTest("unknown_fields_are_ignored_but_unknown_variants_are_rejected") {
                    rustTemplate(
                        """
                        use #{crate}::input::CheckInput;

                        let known: Result<CheckInput, _> = #{serde_json}::from_str(
                            r##"{"kind":"A","choice":{"empty":null},"ignored":true}"##
                        );
                        assert!(known.is_ok(), "known variants should deserialize: {known:?}");

                        let unknown_enum: Result<CheckInput, _> =
                            #{serde_json}::from_str(r##"{"kind":"FUTURE"}"##);
                        assert!(unknown_enum.is_err());

                        let unknown_union: Result<CheckInput, _> =
                            #{serde_json}::from_str(r##"{"choice":{"future":null}}"##);
                        assert!(unknown_union.is_err());

                        let duplicate: Result<CheckInput, _> =
                            #{serde_json}::from_str(r##"{"value":"first","value":"second"}"##);
                        assert!(duplicate.is_err());
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    @Test
    fun testDirectlyAnnotatedLegacyEnumDeserialization() {
        val model =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0

            @awsJson1_0
            service LegacyEnumService {
                operations: [UseLegacyEnum]
            }

            operation UseLegacyEnum {
                input: UseLegacyEnumInput
            }

            structure UseLegacyEnumInput {
                value: LegacyEnum
            }

            @serde(serialize: false, deserialize: true)
            @enum([
                { name: "A", value: "A" }
            ])
            string LegacyEnum
            """.asSmithyModel()

        clientIntegrationTest(
            model,
            IntegrationTestParams(
                service = "com.example#LegacyEnumService",
                cargoCommand = "cargo test --all-features",
            ),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(codegenContext.moduleUseName()),
                    "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                )

            rustCrate.integrationTest("legacy_enum_deserialization") {
                unitTest("directly_annotated_enum_has_deserialize_impl") {
                    rustTemplate(
                        """
                        use #{crate}::types::LegacyEnum;

                        let known: LegacyEnum = #{serde_json}::from_str(r##""A""##)
                            .expect("known enum should deserialize");
                        assert_eq!(known.as_str(), "A");

                        let unknown: LegacyEnum = #{serde_json}::from_str(r##""FUTURE""##)
                            .expect("client unknown enum should be preserved");
                        assert_eq!(unknown.as_str(), "FUTURE");
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    @Test
    fun testLegacySetAndModeledErrorSerde() {
        val model =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0

            @awsJson1_0
            @serde(serialize: true, deserialize: true)
            service LegacyService {
                operations: [UseSet]
            }

            operation UseSet {
                input: UseSetInput
                errors: [CustomError]
            }

            structure UseSetInput {
                tags: Tags
            }

            set Tags {
                member: String
            }

            @error("client")
            structure CustomError {
                message: String
            }
            """.asSmithyModel()

        clientIntegrationTest(
            model,
            IntegrationTestParams(
                service = "com.example#LegacyService",
                cargoCommand = "cargo test --all-features",
            ),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(codegenContext.moduleUseName()),
                    "serde_json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                )

            rustCrate.integrationTest("legacy_set_and_error_serde") {
                unitTest("set_uses_generated_container_and_errors_are_in_the_service_closure") {
                    rustTemplate(
                        """
                        use #{crate}::operation::use_set::UseSetInput;
                        use #{crate}::serde::{SerializationSettings, SerializeConfigured};
                        use #{crate}::types::error::CustomError;

                        let input: UseSetInput = #{serde_json}::from_str(
                            r##"{"tags":["one","two"]}"##
                        ).expect("set should deserialize");
                        let tags = input.tags.expect("tags should be set");
                        assert_eq!(tags.len(), 2);
                        assert_eq!(tags[0], "one");
                        assert_eq!(tags[1], "two");

                        let error: CustomError = #{serde_json}::from_str(
                            r##"{"message":"failed"}"##
                        ).expect("modeled error should deserialize");
                        assert_eq!(error.message(), Some("failed"));

                        let settings = SerializationSettings::default();
                        let serialized = #{serde_json}::to_string(&error.serialize_ref(&settings))
                            .expect("modeled error should serialize");
                        assert_eq!(serialized, r##"{"message":"failed"}"##);
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }

    @Test
    fun testDirectlyAnnotatedEventStreamEnvelopeIsSkipped() {
        val model =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0
            use smithy.framework#ValidationException

            @awsJson1_0
            service EventService {
                operations: [Stream]
            }

            operation Stream {
                input: StreamInput
                errors: [ValidationException]
            }

            @serde(serialize: false, deserialize: true)
            structure StreamInput {
                events: Events
            }

            @streaming
            @serde(serialize: false, deserialize: true)
            union Events {
                message: Message
            }

            structure Message {
                value: String
            }
            """.asSmithyModel(smithyVersion = "2")

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = "com.example#EventService",
                cargoCommand = "cargo test --all-features",
            ),
        ) { _, _ ->
        }
    }
}
