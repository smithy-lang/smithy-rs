/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

description = "Generates Rust/Python code from Smithy models and runs the protocol tests"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server :: Python :: Test"
extra["moduleName"] = "software.amazon.smithy.rust.kotlin.codegen.server.python.test"

tasks["jar"].enabled = false

plugins { id("software.amazon.smithy") }

val smithyVersion: String by project
val defaultRustFlags: String by project
val defaultRustDocFlags: String by project
val properties = PropertyRetriever(rootProject, project)

val pluginName = "rust-server-codegen-python"
val workingDirUnderBuildDir = "smithyprojections/codegen-server-test-python/"

configure<software.amazon.smithy.gradle.SmithyExtension> {
    outputDirectory = file("$buildDir/$workingDirUnderBuildDir")
}

buildscript {
    val smithyVersion: String by project
    dependencies {
        classpath("software.amazon.smithy:smithy-cli:$smithyVersion")
    }
}

dependencies {
    implementation(project(":codegen-server:python"))
    implementation("software.amazon.smithy:smithy-aws-protocol-tests:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
}

val allCodegenTests = listOf(
    CodegenTest("com.amazonaws.simple#SimpleService", "simple"),
    CodegenTest("com.amazonaws.ebs#Ebs", "ebs"),
    CodegenTest("com.aws.example#PokemonService", "pokemon_service_sdk")
)

task("generateSmithyBuild") {
    description = "generate smithy-build.json"
    doFirst {
        projectDir.resolve("smithy-build.json")
            .writeText(
                generateSmithyBuild(
                    rootProject.projectDir.absolutePath,
                    pluginName,
                    codegenTests(properties, allCodegenTests)
                )
            )
    }
}

task("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        buildDir.resolve("$workingDirUnderBuildDir/Cargo.toml")
            .writeText(generateCargoWorkspace(pluginName, codegenTests(properties, allCodegenTests)))
    }
}

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace")

tasks.register<Exec>(Cargo.CHECK.toString) {
    workingDir("$buildDir/$workingDirUnderBuildDir")
    environment("RUSTFLAGS", defaultRustFlags)
    commandLine("cargo", "check")
    dependsOn("assemble")
}

tasks.register<Exec>(Cargo.TEST.toString) {
    workingDir("$buildDir/$workingDirUnderBuildDir")
    environment("RUSTFLAGS", defaultRustFlags)
    commandLine("cargo", "test")
    dependsOn("assemble")
}

tasks.register<Exec>(Cargo.DOCS.toString) {
    workingDir("$buildDir/$workingDirUnderBuildDir")
    environment("RUSTDOCFLAGS", defaultRustDocFlags)
    commandLine("cargo", "doc", "--no-deps")
    dependsOn("assemble")
}

tasks.register<Exec>(Cargo.CLIPPY.toString) {
    workingDir("$buildDir/$workingDirUnderBuildDir")
    environment("RUSTFLAGS", defaultRustFlags)
    commandLine("cargo", "clippy")
    dependsOn("assemble")
}

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }
