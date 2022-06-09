/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

extra["displayName"] = "Smithy :: Rust :: Codegen :: Test"
extra["moduleName"] = "software.amazon.smithy.kotlin.codegen.test"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy").version("0.5.3")
}

val smithyVersion: String by project
val defaultRustFlags: String by project
val defaultRustDocFlags: String by project

dependencies {
    implementation(project(":aws:sdk-codegen"))
    implementation("software.amazon.smithy:smithy-aws-protocol-tests:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
}

data class CodegenTest(val service: String, val module: String, val extraConfig: String? = null)

val CodegenTests = listOf(
    CodegenTest("com.amazonaws.apigateway#BackplaneControlService", "apigateway")
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
                      "moduleAuthors": ["protocoltest@example.com"],
                      "moduleVersion": "0.0.1"
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
        buildDir.resolve("smithy-build.json").writeText(generateSmithyBuild(CodegenTests))
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
        buildDir.resolve("smithyprojections/sdk-codegen-test/Cargo.toml").writeText(generateCargoWorkspace(CodegenTests))
    }
}

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace")

tasks.register<Exec>("cargoCheck") {
    workingDir("build/smithyprojections/sdk-codegen-test/")
    environment("RUSTFLAGS", defaultRustFlags)
    commandLine("cargo", "check")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoTest") {
    workingDir("build/smithyprojections/sdk-codegen-test/")
    environment("RUSTFLAGS", defaultRustFlags)
    commandLine("cargo", "test")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoDocs") {
    workingDir("build/smithyprojections/sdk-codegen-test/")
    environment("RUSTDOCFLAGS", defaultRustDocFlags)
    commandLine("cargo", "doc", "--no-deps")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoClippy") {
    workingDir("build/smithyprojections/sdk-codegen-test/")
    environment("RUSTFLAGS", defaultRustFlags)
    commandLine("cargo", "clippy")
    dependsOn("assemble")
}

tasks["test"].finalizedBy("cargoCheck", "cargoClippy", "cargoTest", "cargoDocs")

tasks["clean"].doFirst {
    delete("smithy-build.json")
}
