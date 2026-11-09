/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Generated SDK variants used only by the manual protocol compatibility matrix.
val protocolCompatibilityCodegenTests =
    listOf(
        // Run identical multi-protocol cases in both AWS JSON response modes.
        *listOf("historical", "specs").map { mode ->
            CodegenTest(
                "smithy.rust.codegen.test.multiprotocol#MultiProtocolService",
                "multi-protocol-server-sdk${if (mode == "specs") {
                    "-specs"
                } else {
                    ""
                }}",
                imports =
                    listOf(
                        "custom-test-models/multi-protocol-compat/multi-protocol-common.smithy",
                        "custom-test-models/multi-protocol-compat/multi-protocol-server.smithy",
                        "custom-test-models/multi-protocol-compat/multi-protocol-awsjson-responses-$mode.smithy",
                    ),
                extraConfig = """, "customizationConfig": {"protocols": {"aws.protocols#awsJson1_0": {"responseBindings": "${if (mode == "specs") {
                    "smithy-specs"
                } else {
                    "historical"
                }}"}, "aws.protocols#awsJson1_1": {"responseBindings": "${if (mode == "specs") {
                    "smithy-specs"
                } else {
                    "historical"
                }}"}}}""",
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
                extraConfig = """, "customizationConfig": {"protocols": {"aws.protocols#awsJson1_0": {"responseBindings": "${if (mode == "specs") {
                    "smithy-specs"
                } else {
                    "historical"
                }}"}, "aws.protocols#awsJson1_1": {"responseBindings": "${if (mode == "specs") {
                    "smithy-specs"
                } else {
                    "historical"
                }}"}}}""",
                extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
            )
        }.toTypedArray(),
        // One service per protocol set, all serving the operations in integration-tests/protocol-compat.
        // Generated serde codegen with a request body limit, to exercise the generated body collection code.
        CodegenTest(
            "com.example.review#ReviewRest",
            "compat-rest-legacy-limit-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraConfig = """, "customizationConfig": {"protocols": {"common": {"request": {"body": {"maxBytes": 1000}}}}}""",
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
        // rpcv2Cbor error with a header-bound member, on schemaSerde and its generated serde twin.
        CodegenTest(
            "com.example.review#ReviewCborErrors",
            "compat-cbor-errors-server-sdk",
            imports = listOf("integration-tests/protocol-compat/models/protocol-compat-cbor-errors.smithy"),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        // Legacy codegen emits non-compiling header serialization for header-bound error
        // members on rpcv2Cbor, so this projection strips the trait the way a customer
        // would have to. Schema serde handles the trait, so its twin above keeps it.
        CodegenTest(
            "com.example.review#ReviewCborErrors",
            "compat-cbor-errors-legacy-server-sdk",
            imports = listOf("integration-tests/protocol-compat/models/protocol-compat-cbor-errors.smithy"),
            // Scope the projection to this service before stripping, because
            // `excludeTraits` applies to the whole assembled model, including the
            // dependency protocol-test models, whose validation relies on the trait.
            transforms =
                listOf(
                    ModelTransform("includeServices", mapOf("services" to listOf("com.example.review#ReviewCborErrors"))),
                    ModelTransform("removeUnusedShapes"),
                    ModelTransform("excludeTraits", mapOf("traits" to listOf("smithy.api#httpHeader"))),
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
        CodegenTest(
            "com.example.review#ReviewRest",
            "compat-rest-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewRestCbor",
            "compat-rest-cbor-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewRestAwsJson",
            "compat-rest-awsjson-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewRestXml",
            "compat-rest-xml-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewAwsJson",
            "compat-awsjson-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewCbor",
            "compat-cbor-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewAll",
            "compat-all-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewDocument",
            "compat-document-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewAwsJson11",
            "compat-awsjson11-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewAwsJson11",
            "compat-awsjson11-legacy-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
        CodegenTest(
            "com.example.review#ReviewXml",
            "compat-xml-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": true""",
        ),
        CodegenTest(
            "com.example.review#ReviewXml",
            "compat-xml-legacy-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/extractor-rejections-protocols.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
        // Generated serde (schemaSerde=false) twins on HTTP 1.x, for wire comparison with the schema services above.
        CodegenTest(
            "com.example.review#ReviewRest",
            "compat-rest-legacy-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
        CodegenTest(
            "com.example.review#ReviewAwsJson",
            "compat-awsjson-legacy-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
        CodegenTest(
            "com.example.review#ReviewCbor",
            "compat-cbor-legacy-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
        CodegenTest(
            "com.example.review#ReviewDocument",
            "compat-document-legacy-server-sdk",
            imports =
                listOf(
                    "integration-tests/protocol-compat/models/protocol-compat-services.smithy",
                    "integration-tests/protocol-compat/models/protocol-compat-operations.smithy",
                ),
            extraCodegenConfig = """"http-1x": true, "schemaSerde": false""",
        ),
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
                    "compat-$model-${if (schemaSerde) {
                        "schema"
                    } else {
                        "generated"
                    }}",
                    imports = listOf("custom-test-models/$model.smithy"),
                    extraCodegenConfig = """"http-1x": true, "schemaSerde": $schemaSerde""",
                )
            }
        }
    } +
        listOf("awsJson1_0", "awsJson1_1", "rpcv2Cbor").flatMap { protocol ->
            listOf(false, true).flatMap { schemaSerde ->
                val test =
                    CodegenTest(
                        "test#Example",
                        "compat-rpc-response-bindings-$protocol-${if (schemaSerde) {
                            "schema"
                        } else {
                            "generated"
                        }}",
                        imports =
                            listOf(
                                "custom-test-models/server-response-test-options.smithy",
                                "custom-test-models/legacy-rpc-response-bindings-$protocol.smithy",
                            ),
                        extraCodegenConfig = """"schemaSerde": $schemaSerde""",
                    )
                if (schemaSerde) {
                    listOf(test.copy(extraCodegenConfig = """"http-1x": true, "schemaSerde": true"""))
                } else {
                    test.bothHttpVersions()
                }
            }
        }

extra["serverCompatibilityCodegenTests"] =
    project.optionalServerCompatibilityTests(protocolCompatibilityCodegenTests + compatibilityProtocolTests)

// These crates are outside the default integration workspace and require optional SDKs.
tasks.register<Exec>("protocolCompatibilityTest") {
    group = "verification"
    description = "Generates the compatibility SDKs and runs the standalone protocol-compat integration suite."
    dependsOn(rootProject.tasks.named("generateServerCompatibilitySdk"))
    workingDir(projectDir.resolve("integration-tests/protocol-compat"))
    commandLine("cargo", "test", "--quiet", "--workspace", "--no-fail-fast")
}

tasks.register<Exec>("compatibilityProtocolTest") {
    group = "verification"
    description = "Runs the optional generated-serde/schema-serde protocol fixture comparisons."
    dependsOn("assemble")
    workingDir(layout.buildDirectory.dir("smithyprojections/codegen-server-test/"))
    commandLine(
        listOf("cargo", "test", "--quiet", "--all-features", "--no-fail-fast") +
            (protocolCompatibilityCodegenTests + compatibilityProtocolTests).flatMap { listOf("--package", it.module.lowercase()) },
    )
}
