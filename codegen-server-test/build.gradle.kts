/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

description = "Generates Rust code from Smithy models and runs the protocol tests"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Test"
extra["moduleName"] = "software.amazon.smithy.rust.kotlin.codegen.server.test"

tasks["jar"].enabled = false

plugins {
    java
    alias(libs.plugins.smithy.gradle.base)
    alias(libs.plugins.smithy.gradle.jar)
}

val properties = PropertyRetriever(rootProject, project)

val pluginName = "rust-server-codegen"
val workingDirUnderBuildDir = "smithyprojections/codegen-server-test/"

dependencies {
    implementation(project(":codegen-server"))
    implementation(project(":codegen-client"))
    implementation(libs.smithy.aws.protocol.tests)
    implementation(libs.smithy.protocol.tests)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.validation.model)
}

smithy {
    format.set(false)
}

val commonCodegenTests = "../codegen-core/common-test-models".let { commonModels ->
    listOf(
        CodegenTest(
            "crate#Config",
            "naming_test_ops",
            imports = listOf("$commonModels/naming-obstacle-course-ops.smithy"),
        ),
        CodegenTest(
            "casing#ACRONYMInside_Service",
            "naming_test_casing",
            imports = listOf("$commonModels/naming-obstacle-course-casing.smithy"),
        ),
        CodegenTest(
            "naming_obs_structs#NamingObstacleCourseStructs",
            "naming_test_structs",
            imports = listOf("$commonModels/naming-obstacle-course-structs.smithy"),
        ),
        CodegenTest(
            "com.amazonaws.simple#SimpleService",
            "simple",
            imports = listOf("$commonModels/simple.smithy"),
        ),
        // Generate both http@0 and http@1 versions for protocol tests
        CodegenTest(
            "smithy.protocoltests.rpcv2Cbor#RpcV2Protocol",
            "rpcv2Cbor",
        ),
        CodegenTest(
            "smithy.protocoltests.rpcv2Cbor#RpcV2Protocol",
            "rpcv2Cbor_schema",
            extraCodegenConfig = """"schemaSerde": true""",
        ),
        CodegenTest(
            "smithy.protocoltests.rpcv2Cbor#RpcV2CborService",
            "rpcv2Cbor_extras",
            imports = listOf("$commonModels/rpcv2Cbor-extras.smithy"),
            extraCodegenConfig = """"alwaysSendEventStreamInitialResponse": true""",
        ),
        CodegenTest(
            "smithy.protocoltests.rpcv2Cbor#RpcV2CborService",
            "rpcv2Cbor_extras_no_initial_response",
            imports = listOf("$commonModels/rpcv2Cbor-extras.smithy"),
        ),
        // Schema-driven path with event streams and initial messages on rpcv2Cbor.
        CodegenTest(
            "smithy.protocoltests.rpcv2Cbor#RpcV2CborService",
            "rpcv2Cbor_extras_schema",
            imports = listOf("$commonModels/rpcv2Cbor-extras.smithy"),
            extraCodegenConfig = """"alwaysSendEventStreamInitialResponse": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "smithy.protocoltests.rpcv2Cbor#RpcV2CborService",
            "rpcv2Cbor_extras_schema_no_initial_response",
            imports = listOf("$commonModels/rpcv2Cbor-extras.smithy"),
            extraCodegenConfig = """"alwaysSendEventStreamInitialResponse": false, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService",
            "constraints_without_public_constrained_types",
            imports = listOf("$commonModels/constraints.smithy"),
            extraCodegenConfig = """"publicConstrainedTypes": false""",
        ),
        CodegenTest(
            "com.amazonaws.constraints#UniqueItemsService",
            "unique_items",
            imports = listOf("$commonModels/unique-items.smithy"),
        ),
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService",
            "constraints",
            imports = listOf("$commonModels/constraints.smithy"),
        ),
        CodegenTest(
            "aws.protocoltests.restjson#RestJson",
            "rest_json",
            extraCodegenConfig = """"debugMode": true""",
        ),
        CodegenTest(
            "aws.protocoltests.restjson#RestJsonExtras",
            "rest_json_extras",
            imports = listOf("$commonModels/rest-json-extras.smithy"),
        ),
        // Schema-driven (runtime) request/response path for restJson1.
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService",
            "constraints_schema",
            imports = listOf("$commonModels/constraints.smithy"),
            extraCodegenConfig = """"schemaSerde": true""",
        ),
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService",
            "constraints_without_public_constrained_types_schema",
            imports = listOf("$commonModels/constraints.smithy"),
            extraCodegenConfig = """"publicConstrainedTypes": false, "schemaSerde": true""",
        ),
        CodegenTest(
            "aws.protocoltests.restjson#RestJson",
            "rest_json_schema",
            extraCodegenConfig = """"schemaSerde": true""",
        ),
        CodegenTest(
            "aws.protocoltests.restjson#RestJsonExtras",
            "rest_json_extras_schema",
            imports = listOf("$commonModels/rest-json-extras.smithy"),
            extraCodegenConfig = """"schemaSerde": true""",
        ),
        CodegenTest(
            "aws.protocoltests.restjson.validation#RestJsonValidation",
            "rest_json_validation_schema",
            extraCodegenConfig = """"ignoreUnsupportedConstraints": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "aws.protocoltests.restjson.validation#RestJsonValidation",
            "rest_json_validation",
            // `@range` trait is used on floating point shapes, which we deliberately don't want to support.
            // See https://github.com/smithy-lang/smithy-rs/issues/1401.
            extraCodegenConfig = """"ignoreUnsupportedConstraints": true""",
        ),
        CodegenTest(
            "aws.protocoltests.json10#JsonRpc10",
            "json_rpc10",
        ),
        CodegenTest(
            "aws.protocoltests.json10#JsonRpc10",
            "json_rpc10_schema",
            extraCodegenConfig = """"schemaSerde": true""",
        ),
        CodegenTest(
            "aws.protocoltests.json#JsonProtocol",
            "json_rpc11",
        ),
        CodegenTest(
            "aws.protocoltests.json#JsonProtocol",
            "json_rpc11_schema",
            extraCodegenConfig = """"schemaSerde": true""",
        ),
        CodegenTest(
            "aws.protocoltests.misc#MiscService",
            "misc",
            imports = listOf("$commonModels/misc.smithy"),
        ),
        CodegenTest(
            "com.amazonaws.ebs#Ebs",
            "ebs",
            imports = listOf("$commonModels/ebs.json"),
        ),
        CodegenTest(
            "com.amazonaws.s3#AmazonS3",
            "s3",
        ),
        CodegenTest(
            "com.aws.example#PokemonService",
            "pokemon-service-server-sdk",
            imports = listOf("$commonModels/pokemon.smithy", "$commonModels/pokemon-common.smithy"),
            extraCodegenConfig = """"debugMode": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.aws.example#PokemonService",
            "pokemon-service-awsjson-server-sdk",
            imports = listOf("$commonModels/pokemon-awsjson.smithy", "$commonModels/pokemon-common.smithy"),
        ),
        CodegenTest(
            "com.aws.example#PokemonService",
            "pokemon-service-legacy-server-sdk",
            imports = listOf("$commonModels/pokemon.smithy", "$commonModels/pokemon-common.smithy"),
            extraCodegenConfig = """"debugMode": true, "schemaSerde": false""",
        ),
        CodegenTest(
            "com.aws.example#PokemonService",
            "pokemon-service-schema-server-sdk",
            imports = listOf("$commonModels/pokemon.smithy", "$commonModels/pokemon-common.smithy"),
            extraCodegenConfig = """"schemaSerde": true""",
        ),
    ).flatMap { it.bothHttpVersions() }.map {
        if (it.module.endsWith("-http0x")) {
            // Generated serde projections must disable schema serde, which otherwise enables HTTP 1.x.
            it.copy(extraCodegenConfig = it.extraCodegenConfig?.replace(""""schemaSerde": true""", """"schemaSerde": false"""))
        } else if (it.module == "pokemon-service-server-sdk") {
            // Only the schema-driven HTTP/1.x example serves multiple protocols.
            it.copy(imports = it.imports + "custom-test-models/pokemon-multi-protocol.smithy")
        } else {
            it
        }
    } +
        listOf(
            CodegenTest(
                "smithy.rust.server.protocoltests#RestXmlServerTests",
                "rest_xml_schema",
                imports = listOf("custom-test-models/rest-xml-server-tests.smithy"),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            // Run identical multi-protocol cases in both AWS JSON response modes.
            *listOf("historical", "specs").map { mode ->
                CodegenTest(
                    "smithy.rust.codegen.test.multiprotocol#MultiProtocolService",
                    "multi-protocol-server-sdk${if (mode == "specs") "-specs" else ""}",
                    imports =
                        listOf(
                            "custom-test-models/multi-protocol-compat/multi-protocol-common.smithy",
                            "custom-test-models/multi-protocol-compat/multi-protocol-server.smithy",
                            "custom-test-models/multi-protocol-compat/multi-protocol-awsjson-responses-$mode.smithy",
                        ),
                    extraConfig = """, "customizationConfig": {"protocols": {"aws.protocols#awsJson1_0": {"responseBindings": "${if (mode == "specs") "smithy-specs" else "historical"}"}, "aws.protocols#awsJson1_1": {"responseBindings": "${if (mode == "specs") "smithy-specs" else "historical"}"}}}""",
                    extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
                )
            }.toTypedArray(),
            // JSON-only document cases distinguish None from Some(Document::Null) in both response modes.
            *listOf("historical", "specs").map { mode ->
                CodegenTest(
                    "smithy.rust.codegen.test.multiprotocol#MultiProtocolService",
                    "multi-protocol-document-server-sdk-$mode",
                    imports =
                        listOf(
                            "custom-test-models/multi-protocol-compat/multi-protocol-common.smithy",
                            "custom-test-models/multi-protocol-compat/multi-protocol-document-server.smithy",
                            "custom-test-models/multi-protocol-compat/multi-protocol-awsjson-responses-$mode.smithy",
                        ),
                    extraConfig = """, "customizationConfig": {"protocols": {"aws.protocols#awsJson1_0": {"responseBindings": "${if (mode == "specs") "smithy-specs" else "historical"}"}, "aws.protocols#awsJson1_1": {"responseBindings": "${if (mode == "specs") "smithy-specs" else "historical"}"}}}""",
                    extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
                )
            }.toTypedArray(),
            // One service per protocol set, all serving the operations in integration-tests/protocol-compat.
            // Generated serde codegen with a request body limit, to exercise the generated body collection code.
            CodegenTest(
                "com.example.review#ReviewRest",
                "compat-rest-legacy-limit-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraConfig = """, "customizationConfig": {"protocols": {"common": {"requestBodyMaxBytes": 1000}}}""",
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
            // rpcv2Cbor error with a header-bound member, on schemaSerde and its generated serde twin.
            CodegenTest(
                "com.example.review#ReviewCborErrors",
                "compat-cbor-errors-server-sdk",
                imports = listOf("integration-tests/protocol-compat/models/protocol-compat-cbor-errors.smithy"),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewCborErrors",
                "compat-cbor-errors-legacy-server-sdk",
                imports = listOf("integration-tests/protocol-compat/models/protocol-compat-cbor-errors.smithy"),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
            CodegenTest(
                "com.example.review#ReviewRest",
                "compat-rest-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewRestCbor",
                "compat-rest-cbor-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewRestAwsJson",
                "compat-rest-awsjson-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewRestXml",
                "compat-rest-xml-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewAwsJson",
                "compat-awsjson-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewCbor",
                "compat-cbor-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewAll",
                "compat-all-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewDocument",
                "compat-document-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            // awsJson following the specification (`responseBindings: "smithy-specs"`) instead of the historical default.
            CodegenTest(
                "com.example.review#ReviewAwsJson",
                "compat-awsjson-spec-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraConfig = """, "customizationConfig": {"protocols": {"aws.protocols#awsJson1_0": {"responseBindings": "smithy-specs"}}}""",
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewAwsJson11",
                "compat-awsjson11-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewAwsJson11",
                "compat-awsjson11-legacy-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
            CodegenTest(
                "com.example.review#ReviewXml",
                "compat-xml-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            ),
            CodegenTest(
                "com.example.review#ReviewXml",
                "compat-xml-legacy-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
            // Generated serde (schemaSerde=false) twins on HTTP 1.x, for wire comparison with the schema services above.
            CodegenTest(
                "com.example.review#ReviewRest",
                "compat-rest-legacy-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
            CodegenTest(
                "com.example.review#ReviewAwsJson",
                "compat-awsjson-legacy-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
            CodegenTest(
                "com.example.review#ReviewCbor",
                "compat-cbor-legacy-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
            CodegenTest(
                "com.example.review#ReviewDocument",
                "compat-document-legacy-server-sdk",
                imports = listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
                extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
            ),
        )
}
// When iterating on protocol tests use this to speed up codegen:
//    .filter { it.module == "rpcv2Cbor_extras" || it.module == "rpcv2Cbor_extras_no_initial_response" }

val customCodegenTests = "custom-test-models".let { customModels ->
    CodegenTest(
        "com.aws.example#CustomValidationExample",
        "custom-validation-exception-example",
        imports = listOf("$customModels/custom-validation-exception.smithy"),
    ).bothHttpVersions()
}

// Server-only Criterion comparisons use the same model and HTTP types on both serde paths.
val eventStreamBenchmarkTests =
    listOf("rest-json", "rpc-v2-cbor").flatMap { protocol ->
        listOf(false, true).map { schemaSerde ->
            CodegenTest(
                "benchmark.eventstream#BenchmarkService",
                "event-stream-bench-$protocol-${if (schemaSerde) "schema" else "legacy"}",
                imports =
                    listOf(
                        "benchmarks/event-stream/model/common.smithy",
                        "benchmarks/event-stream/model/$protocol.smithy",
                    ),
                extraCodegenConfig =
                    """"http-1x": true, "schemaSerde": $schemaSerde, "alwaysSendEventStreamInitialResponse": ${protocol == "rpc-v2-cbor"}""",
            )
        }
    } +
        // Match the selected single-protocol service's initial-response setting.
        // The global setting must be false for REST and true for CBOR, so the
        // two multi-protocol projections differ only in that setting.
        CodegenTest(
            "benchmark.eventstream#BenchmarkService",
            "event-stream-bench-multi-rest-json-schema",
            imports =
                listOf(
                    "benchmarks/event-stream/model/common.smithy",
                    "benchmarks/event-stream/model/multi.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true, "alwaysSendEventStreamInitialResponse": false""",
        ) +
        CodegenTest(
            "benchmark.eventstream#BenchmarkService",
            "event-stream-bench-multi-schema",
            imports =
                listOf(
                    "benchmarks/event-stream/model/common.smithy",
                    "benchmarks/event-stream/model/multi.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true, "alwaysSendEventStreamInitialResponse": true""",
        )

// Run the same server compatibility fixtures through generated serde and schema serde.
val compatibilityProtocolTests =
    listOf("restJson1", "restXml").flatMap { protocol ->
        listOf(
            "body-bindings-$protocol" to "test.bodybindings#Orders",
            "collection-timestamps-$protocol" to "test.collectiontimestamps#Times",
            "prefix-headers-$protocol-plain" to "test.prefixwhitespace#Headers",
            "prefix-headers-$protocol-media" to "test.prefixwhitespace#Headers",
        ).flatMap { (model, service) ->
            listOf(false, true).map { schemaSerde ->
                CodegenTest(
                    service,
                    "compat-$model-${if (schemaSerde) "schema" else "generated"}",
                    imports = listOf("schema-serde-compat/protocol-tests/$model.smithy"),
                    extraCodegenConfig = """"http-1x": true, "schemaSerde": $schemaSerde""",
                )
            }
        }
    }

// Clients for the server compatibility suite and multiprotocol example share the server-owned models.
val compatibilityClientTests =
    listOf(
        "multi-protocol-client" to "restjson",
        "multi-protocol-client-rpcv2cbor" to "rpcv2cbor",
        "multi-protocol-client-awsjson10" to "awsjson10",
        "multi-protocol-client-awsjson11" to "awsjson11",
        "multi-protocol-client-restjson" to "restjson",
        "multi-protocol-client-restxml" to "restxml",
    ).map { (module, protocol) ->
        CodegenTest(
            "smithy.rust.codegen.test.multiprotocol#MultiProtocolService",
            module,
            imports =
                listOf(
                    "custom-test-models/multi-protocol-compat/multi-protocol-$protocol.smithy",
                    "custom-test-models/multi-protocol-compat/multi-protocol-common.smithy",
                ),
            extraCodegenConfig =
                """"addMessageToErrors": true, "renameErrors": true, "enableNewSmithyRuntime": "${properties.get("smithy.runtime.mode") ?: "orchestrator"}"""",
            pluginName = "rust-client-codegen",
        )
    }

val allCodegenTests = commonCodegenTests + customCodegenTests + eventStreamBenchmarkTests + compatibilityProtocolTests + compatibilityClientTests

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)

tasks["generateSmithyBuild"].inputs.property("smithy.runtime.mode", properties.get("smithy.runtime.mode") ?: "orchestrator")

tasks["smithyBuild"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace", "generateCargoConfigToml")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile, useNextest = true)

tasks.register<Exec>("cargoTestIntegration") {
    dependsOn("assemble")
    workingDir(projectDir.resolve("integration-tests"))
    commandLine("cargo", "nextest", "run", "--no-tests=pass")
}

// `cargo nextest run` does not run doctests, so run them separately to preserve coverage.
tasks.register<Exec>("cargoTestIntegrationDoctests") {
    dependsOn("assemble")
    workingDir(projectDir.resolve("integration-tests"))
    commandLine("cargo", "test", "--doc")
}
tasks["cargoTestIntegration"].finalizedBy("cargoTestIntegrationDoctests")

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString }, "cargoTestIntegration")

tasks["clean"].doFirst { delete("smithy-build.json") }
