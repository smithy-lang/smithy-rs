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
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

class SerdeProtocolTestTest {
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
    fun testLegacySetAndModeledErrorDeserialization() {
        val model =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0

            @awsJson1_0
            @serde(serialize: false, deserialize: true)
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

            rustCrate.integrationTest("legacy_set_and_error_deserialization") {
                unitTest("set_uses_generated_container_and_errors_are_in_the_service_closure") {
                    rustTemplate(
                        """
                        use #{crate}::operation::use_set::UseSetInput;
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
