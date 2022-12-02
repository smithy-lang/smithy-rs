/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

description = "Generates Rust code from Smithy models and runs the protocol tests"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Test"
extra["moduleName"] = "software.amazon.smithy.rust.kotlin.codegen.server.test"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy")
}

val smithyVersion: String by project
val defaultRustDocFlags: String by project
val properties = PropertyRetriever(rootProject, project)

val pluginName = "rust-server-codegen"
val workingDirUnderBuildDir = "smithyprojections/codegen-server-test/"

buildscript {
    val smithyVersion: String by project
    dependencies {
        classpath("software.amazon.smithy:smithy-cli:$smithyVersion")
    }
}

dependencies {
    implementation(project(":codegen-server"))
    implementation("software.amazon.smithy:smithy-aws-protocol-tests:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-validation-model:$smithyVersion")
}

val allCodegenTests = "../codegen-core/common-test-models".let { commonModels ->
    listOf(
        CodegenTest("crate#Config", "naming_test_ops", imports = listOf("$commonModels/naming-obstacle-course-ops.smithy")),
        CodegenTest(
            "naming_obs_structs#NamingObstacleCourseStructs",
            "naming_test_structs",
            imports = listOf("$commonModels/naming-obstacle-course-structs.smithy"),
        ),
        CodegenTest("com.amazonaws.simple#SimpleService", "simple", imports = listOf("$commonModels/simple.smithy")),
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService", "constraints_without_public_constrained_types",
            imports = listOf("$commonModels/constraints.smithy"),
            extraConfig = """, "codegen": { "publicConstrainedTypes": false } """,
        ),
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService",
            "constraints",
            imports = listOf("$commonModels/constraints.smithy"),
        ),
        CodegenTest(
            "aws.protocoltests.restjson#RestJson",
            "rest_json",
            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) `@uniqueItems` is used.
            extraConfig = """, "codegen": { "ignoreUnsupportedConstraints": true } """,
        ),
        CodegenTest(
            "aws.protocoltests.restjson#RestJsonExtras",
            "rest_json_extras",
            imports = listOf("$commonModels/rest-json-extras.smithy"),
        ),
        CodegenTest(
            "aws.protocoltests.restjson.validation#RestJsonValidation", "rest_json_validation",
            extraConfig = """, "codegen": { "ignoreUnsupportedConstraints": true } """,
        ),
        CodegenTest(
            "aws.protocoltests.extras.restjson.validation#MalformedRangeValidation", "malformed_range_extras",
            extraConfig = """, "codegen": { "ignoreUnsupportedConstraints": true } """,
            imports = listOf("$commonModels/malformed-range-extras.smithy"),
        ),
        CodegenTest("aws.protocoltests.json10#JsonRpc10", "json_rpc10"),
        CodegenTest(
            "aws.protocoltests.json#JsonProtocol",
            "json_rpc11",
            extraConfig = """, "codegen": { "ignoreUnsupportedConstraints": true } """,
        ),
        CodegenTest(
            "aws.protocoltests.misc#MiscService",
            "misc",
            imports = listOf("$commonModels/misc.smithy"),
            // TODO(https://github.com/awslabs/smithy-rs/issues/1401) `@uniqueItems` is used.
            extraConfig = """, "codegen": { "ignoreUnsupportedConstraints": true } """,
        ),
        CodegenTest(
            "com.amazonaws.ebs#Ebs", "ebs",
            imports = listOf("$commonModels/ebs.json"),
            extraConfig = """, "codegen": { "ignoreUnsupportedConstraints": true } """,
        ),
        CodegenTest("com.amazonaws.s3#AmazonS3", "s3"),
        CodegenTest(
            "com.aws.example.rust#PokemonService",
            "pokemon-service-server-sdk",
            imports = listOf("$commonModels/pokemon.smithy", "$commonModels/pokemon-common.smithy"),
        ),
    )
}

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(buildDir.resolve(workingDirUnderBuildDir))

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace", "generateCargoConfigToml")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(buildDir.resolve(workingDirUnderBuildDir), defaultRustDocFlags)

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }
