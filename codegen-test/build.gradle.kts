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
val properties = PropertyRetriever(rootProject, project)

val pluginName = "rust-codegen"
val workingDirUnderBuildDir = "smithyprojections/codegen-test/"

buildscript {
    val smithyVersion: String by project
    dependencies {
        classpath("software.amazon.smithy:smithy-cli:$smithyVersion")
    }
}

dependencies {
    implementation(project(":codegen"))
    implementation("software.amazon.smithy:smithy-aws-protocol-tests:$smithyVersion")
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
}

val allCodegenTests = listOf(
    CodegenTest("com.amazonaws.simple#SimpleService", "simple"),
    CodegenTest("com.amazonaws.dynamodb#DynamoDB_20120810", "dynamo"),
    CodegenTest("com.amazonaws.ebs#Ebs", "ebs"),
    CodegenTest("aws.protocoltests.json10#JsonRpc10", "json_rpc10"),
    CodegenTest("aws.protocoltests.json#JsonProtocol", "json_rpc11"),
    CodegenTest("aws.protocoltests.restjson#RestJson", "rest_json"),
    CodegenTest("aws.protocoltests.restjson#RestJsonExtras", "rest_json_extras"),
    CodegenTest("aws.protocoltests.misc#MiscService", "misc"),
    CodegenTest(
        "aws.protocoltests.restxml#RestXml", "rest_xml",
        extraConfig = """, "codegen": { "addMessageToErrors": false } """
    ),

    CodegenTest(
        "aws.protocoltests.query#AwsQuery", "aws_query",
        extraConfig = """, "codegen": { "addMessageToErrors": false } """
    ),
    CodegenTest(
        "aws.protocoltests.ec2#AwsEc2", "ec2_query",
        extraConfig = """, "codegen": { "addMessageToErrors": false } """
    ),
    CodegenTest(
        "aws.protocoltests.restxml.xmlns#RestXmlWithNamespace",
        "rest_xml_namespace",
        extraConfig = """, "codegen": { "addMessageToErrors": false } """
    ),
    CodegenTest(
        "aws.protocoltests.restxml#RestXmlExtras",
        "rest_xml_extras",
        extraConfig = """, "codegen": { "addMessageToErrors": false } """
    ),
    CodegenTest(
        "aws.protocoltests.restxmlunwrapped#RestXmlExtrasUnwrappedErrors",
        "rest_xml_extras_unwrapped",
        extraConfig = """, "codegen": { "addMessageToErrors": false } """
    ),
    CodegenTest(
        "crate#Config",
        "naming_test_ops",
        """
            , "codegen": { "renameErrors": false }
        """.trimIndent()
    ),
    CodegenTest(
        "naming_obs_structs#NamingObstacleCourseStructs",
        "naming_test_structs",
        """
            , "codegen": { "renameErrors": false }
        """.trimIndent()
    ),
    CodegenTest("com.aws.example#PokemonService", "pokemon_service_client")
)

tasks.register("generateSmithyBuild") {
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

tasks.register("generateCargoWorkspace") {
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

tasks["clean"].doFirst {
    delete("smithy-build.json")
}
