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
            "aws.protocoltests.json#JsonProtocol",
            "json_rpc11",
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
            extraCodegenConfig = """"debugMode": true""",
        ),
        CodegenTest(
            "com.aws.example#PokemonService",
            "pokemon-service-awsjson-server-sdk",
            imports = listOf("$commonModels/pokemon-awsjson.smithy", "$commonModels/pokemon-common.smithy"),
        ),
    ).flatMap { it.bothHttpVersions() }
}
// When iterating on protocol tests use this to speed up codegen:
// .filter { it.module == "rpcv2Cbor_extras" || it.module == "rpcv2Cbor_extras_no_initial_response" }

val customCodegenTests = "custom-test-models".let { customModels ->
    CodegenTest(
        "com.aws.example#CustomValidationExample",
        "custom-validation-exception-example",
        imports = listOf("$customModels/custom-validation-exception.smithy"),
    ).bothHttpVersions()
}

val allCodegenTests = commonCodegenTests + customCodegenTests

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)

tasks["smithyBuild"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace", "generateCargoConfigToml")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)

tasks.register<Exec>("cargoTestIntegration") {
    dependsOn("assemble")
    workingDir(projectDir.resolve("integration-tests"))
    commandLine("cargo", "test")
}

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString }, "cargoTestIntegration")

tasks["clean"].doFirst { delete("smithy-build.json") }
