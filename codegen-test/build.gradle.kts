/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

extra["displayName"] = "Smithy :: Rust :: Codegen :: Test"
extra["moduleName"] = "software.amazon.smithy.kotlin.codegen.test"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy").version("0.5.2")
}

val smithyVersion: String by project


dependencies {
    implementation(project(":codegen"))
    implementation("software.amazon.smithy:smithy-aws-protocol-tests:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
}

data class CodegenTest(val service: String, val module: String, val extraConfig: String? = null)

val CodegenTests = listOf(
    CodegenTest("com.amazonaws.dynamodb#DynamoDB_20120810", "dynamo"),
    CodegenTest("com.amazonaws.ebs#Ebs", "ebs"),
    CodegenTest("aws.protocoltests.json10#JsonRpc10", "json_rpc10"),
    CodegenTest(
        "aws.protocoltests.json#JsonProtocol",
        "json_rpc11"
    ),
    CodegenTest(
        "aws.protocoltests.restjson#RestJson",
        "rest_json"
    ),
    CodegenTest(
        "aws.protocoltests.restjson#RestJsonExtras",
        "rest_json_extas"
    ),
    CodegenTest(
        "crate#Config",
        "naming_test", """
            , "codegen": { "renameErrors": false }
        """.trimIndent()
    )
)

fun generateSmithyBuild(tests: List<CodegenTest>): String {
    val projections = tests.joinToString(",\n") {
        """
            "${it.module}": {
                "plugins": {
                    "rust-codegen": {
                      "runtimeConfig": {
                        "relativePath": "${rootProject.projectDir.absolutePath}/rust-runtime"
                      },
                      "service": "${it.service}",
                      "module": "${it.module}",
                      "moduleVersion": "0.0.1",
                      "build": {
                        "rootProject": true
                      }
                      ${it.extraConfig ?: ""}
                 }
               }
            }
        """.trimIndent()
    }
    return """
    {
        "version": "1.0",
        "projections": { $projections }
    }
    """
}


task("generateSmithyBuild") {
    description = "generate smithy-build.json"
    doFirst {
        projectDir.resolve("smithy-build.json").writeText(generateSmithyBuild(CodegenTests))
    }
}


fun generateCargoWorkspace(tests: List<CodegenTest>): String {
    return """
    [workspace]
    members = [
        ${tests.joinToString(",") { "\"${it.module}/rust-codegen\"" }}
    ]
    """.trimIndent()
}
task("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        buildDir.resolve("smithyprojections/codegen-test/Cargo.toml").writeText(generateCargoWorkspace(CodegenTests))
    }
}

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace")


tasks.register<Exec>("cargoCheck") {
    workingDir("build/smithyprojections/codegen-test/")
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "check")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoTest") {
    workingDir("build/smithyprojections/codegen-test/")
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "test")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoDocs") {
    workingDir("build/smithyprojections/codegen-test/")
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "doc", "--no-deps")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoClippy") {
    workingDir("build/smithyprojections/codegen-test/")
    // disallow warnings
    commandLine("cargo", "clippy", "--", "-D", "warnings", "-Aclippy::upper_case_acronyms", "-Aclippy::large-enum-variant")
    dependsOn("assemble")
}

tasks["test"].finalizedBy("cargoCheck", "cargoClippy", "cargoTest", "cargoDocs")

tasks["clean"].doFirst {
    delete("smithy-build.json")
}
