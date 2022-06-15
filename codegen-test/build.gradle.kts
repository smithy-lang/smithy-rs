/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

extra["displayName"] = "Smithy :: Rust :: Codegen :: Test"
extra["moduleName"] = "software.amazon.smithy.kotlin.codegen.test"

tasks["jar"].enabled = false

plugins { id("software.amazon.smithy").version("0.5.3") }

val smithyVersion: String by project
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

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(buildDir.resolve(workingDirUnderBuildDir))

tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(buildDir.resolve(workingDirUnderBuildDir), defaultRustDocFlags)

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }
