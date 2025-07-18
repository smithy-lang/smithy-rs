/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    java
    alias(libs.plugins.smithy.gradle.base)
    alias(libs.plugins.smithy.gradle.jar)
}

description = "Generates Rust/Python code from Smithy models and runs the protocol tests"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Python :: Test"
extra["moduleName"] = "software.amazon.smithy.rust.kotlin.codegen.server.python.test"

tasks.jar.configure {
    enabled = false
}

val properties = PropertyRetriever(rootProject, project)
val buildDir = layout.buildDirectory.get().asFile

val pluginName = "rust-server-codegen-python"
val workingDirUnderBuildDir = "smithyprojections/codegen-server-test-python/"

smithy {
    outputDirectory = layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile
    format = false
}

dependencies {
    implementation(project(":codegen-server:python"))
    implementation(libs.smithy.aws.protocol.tests)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.aws.traits)
}

val allCodegenTests = "../../codegen-core/common-test-models".let { commonModels ->
    listOf(
        CodegenTest("com.amazonaws.simple#SimpleService", "simple", imports = listOf("$commonModels/simple.smithy")),
        CodegenTest(
            "com.aws.example#PokemonService",
            "pokemon-service-server-sdk",
            imports = listOf("$commonModels/pokemon.smithy", "$commonModels/pokemon-common.smithy"),
        ),
        CodegenTest(
            "com.amazonaws.ebs#Ebs",
            "ebs",
            imports = listOf("$commonModels/ebs.json"),
        ),
        CodegenTest(
            "aws.protocoltests.misc#MiscService",
            "misc",
            imports = listOf("$commonModels/misc.smithy"),
        ),
        CodegenTest(
            "aws.protocoltests.json#JsonProtocol",
            "json_rpc11",
        ),
        CodegenTest("aws.protocoltests.json10#JsonRpc10", "json_rpc10"),
        CodegenTest("aws.protocoltests.restjson#RestJson", "rest_json"),
        CodegenTest(
            "aws.protocoltests.restjson#RestJsonExtras",
            "rest_json_extras",
            imports = listOf("$commonModels/rest-json-extras.smithy"),
        ),
        // TODO(https://github.com/smithy-lang/smithy-rs/issues/2477)
        // CodegenTest(
        //     "aws.protocoltests.restjson.validation#RestJsonValidation",
        //     "rest_json_validation",
        //     // `@range` trait is used on floating point shapes, which we deliberately don't want to support.
        //     // See https://github.com/smithy-lang/smithy-rs/issues/1401.
        //     extraConfig = """, "codegen": { "ignoreUnsupportedConstraints": true } """,
        // ),
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService",
            "constraints",
            imports = listOf("$commonModels/constraints.smithy"),
        ),
        CodegenTest(
            "com.amazonaws.constraints#ConstraintsService",
            "constraints_without_public_constrained_types",
            imports = listOf("$commonModels/constraints.smithy"),
            extraConfig = """, "codegen": { "publicConstrainedTypes": false } """,
        ),
        CodegenTest(
            "com.amazonaws.constraints#UniqueItemsService",
            "unique_items",
            imports = listOf("$commonModels/unique-items.smithy"),
        ),
        CodegenTest(
            "naming_obs_structs#NamingObstacleCourseStructs",
            "naming_test_structs",
            imports = listOf("$commonModels/naming-obstacle-course-structs.smithy"),
        ),
        CodegenTest("casing#ACRONYMInside_Service", "naming_test_casing", imports = listOf("$commonModels/naming-obstacle-course-casing.smithy")),
        CodegenTest("crate#Config", "naming_test_ops", imports = listOf("$commonModels/naming-obstacle-course-ops.smithy")),
    )
}

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(buildDir.resolve(workingDirUnderBuildDir))

tasks.register("stubs") {
    description = "Generate Python stubs for all models"
    dependsOn("assemble")

    doLast {
        allCodegenTests.forEach { test ->
            val crateDir = layout.buildDirectory.dir("$workingDirUnderBuildDir/${test.module}/$pluginName").get().asFile.path
            val moduleName = test.module.replace("-", "_")
            exec {
                commandLine("bash", "$crateDir/stubgen.sh", moduleName, "$crateDir/Cargo.toml", "$crateDir/python/$moduleName")
            }
        }
    }
}

tasks.smithyBuild.configure {
    dependsOn("generateSmithyBuild")
}
tasks.assemble.configure {
    finalizedBy("generateCargoWorkspace")
}

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(buildDir.resolve(workingDirUnderBuildDir))

tasks.test.configure {
    finalizedBy(cargoCommands(properties).map { it.toString })
}

tasks.clean.configure {
    doFirst {
        delete("smithy-build.json")
    }
}
