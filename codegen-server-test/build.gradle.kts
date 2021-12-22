/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Test"

extra["moduleName"] = "software.amazon.smithy.rust.kotlin.codegen.server.test"

tasks["jar"].enabled = false

plugins { id("software.amazon.smithy").version("0.5.3") }

val smithyVersion: String by project

dependencies {
    implementation(project(":codegen-server"))
    implementation("software.amazon.smithy:smithy-aws-protocol-tests:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
}

data class CodegenTest(val service: String, val module: String, val extraConfig: String? = null)

val CodegenTests = listOf(
    CodegenTest("com.amazonaws.simple#SimpleService", "simple"),
    CodegenTest("aws.protocoltests.restjson#RestJson", "rest_json"),
    CodegenTest("com.amazonaws.ebs#Ebs", "ebs"),
    CodegenTest("com.amazonaws.s3#AmazonS3", "s3")
)

/**
 * `includeFluentClient` must be set to `false` as we are not generating all the supporting
 * code for it.
 * TODO: Review how can we make this a default in the server so that customers don't
 *       have to specify it.
 */
fun generateSmithyBuild(tests: List<CodegenTest>): String {
    val projections =
        tests.joinToString(",\n") {
            """
            "${it.module}": {
                "plugins": {
                    "rust-server-codegen": {
                      "runtimeConfig": {
                        "relativePath": "${rootProject.projectDir.absolutePath}/rust-runtime"
                      },
                      "service": "${it.service}",
                      "module": "${it.module}",
                      "moduleVersion": "0.0.1",
                      "moduleDescription": "test",
                      "moduleAuthors": ["protocoltest@example.com"]
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
    doFirst { projectDir.resolve("smithy-build.json").writeText(generateSmithyBuild(CodegenTests)) }
}

fun generateCargoWorkspace(tests: List<CodegenTest>): String {
    return """
    [workspace]
    members = [
        ${tests.joinToString(",") { "\"${it.module}/rust-server-codegen\"" }}
    ]
    """.trimIndent()
}

task("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        buildDir.resolve("smithyprojections/codegen-server-test/Cargo.toml")
            .writeText(generateCargoWorkspace(CodegenTests))
    }
}

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].dependsOn("smithyBuildJar")
tasks["assemble"].finalizedBy("generateCargoWorkspace")

tasks.register<Exec>("cargoCheck") {
    workingDir("build/smithyprojections/codegen-server-test/")
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "check")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoTest") {
    workingDir("build/smithyprojections/codegen-server-test/")
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "test")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoDocs") {
    workingDir("build/smithyprojections/codegen-server-test/")
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "doc", "--no-deps")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoClippy") {
    workingDir("build/smithyprojections/codegen-server-test/")
    // disallow warnings
    commandLine("cargo", "clippy", "--", "-D", "warnings")
    dependsOn("assemble")
}

tasks["test"].finalizedBy("cargoCheck", "cargoClippy", "cargoTest", "cargoDocs")

tasks["clean"].doFirst { delete("smithy-build.json") }
