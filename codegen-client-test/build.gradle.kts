/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

extra["displayName"] = "Smithy :: Rust :: Codegen :: Test"
extra["moduleName"] = "software.amazon.smithy.kotlin.codegen.test"

tasks["jar"].enabled = false

plugins {
    java
    id("software.amazon.smithy.gradle.smithy-base")
    id("software.amazon.smithy.gradle.smithy-jar")
}

val properties = PropertyRetriever(rootProject, project)
fun getSmithyRuntimeMode(): String = properties.get("smithy.runtime.mode") ?: "orchestrator"

val pluginName = "rust-client-codegen"
val workingDirUnderBuildDir = "smithyprojections/codegen-client-test/"

val checkedInSmithyRuntimeLockfile = rootProject.projectDir.resolve("rust-runtime/Cargo.lock")

dependencies {
    implementation(project(":codegen-client"))
    implementation(libs.smithy.aws.protocol.tests)
    implementation(libs.smithy.protocol.tests)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.aws.traits)
}

// Disabled because the formatter was remove formatting from our `body` sections.
smithy {
    format.set(false)
}

data class ClientTest(
    val serviceShapeName: String,
    val moduleName: String,
    val dependsOn: List<String> = emptyList(),
    val addMessageToErrors: Boolean = true,
    val renameErrors: Boolean = true,
) {
    fun toCodegenTest(): CodegenTest = CodegenTest(
        serviceShapeName,
        moduleName,
        extraCodegenConfig = extraCodegenConfig(),
        imports = imports(),
    )

    private fun extraCodegenConfig(): String = StringBuilder().apply {
        append("\"addMessageToErrors\": $addMessageToErrors,\n")
        append("\"renameErrors\": $renameErrors\n,")
        append("\"enableNewSmithyRuntime\": \"${getSmithyRuntimeMode()}\"")
    }.toString()

    private fun imports(): List<String> = dependsOn.map { "../codegen-core/common-test-models/$it" }
}

val allCodegenTests = listOf(
    ClientTest("com.amazonaws.simple#SimpleService", "simple", dependsOn = listOf("simple.smithy")),
    ClientTest("com.amazonaws.dynamodb#DynamoDB_20120810", "dynamo"),
    ClientTest("com.amazonaws.ebs#Ebs", "ebs", dependsOn = listOf("ebs.json")),
    ClientTest("aws.protocoltests.json10#JsonRpc10", "json_rpc10"),
    ClientTest("aws.protocoltests.json#JsonProtocol", "json_rpc11"),
    ClientTest("aws.protocoltests.restjson#RestJson", "rest_json"),
    ClientTest(
        "aws.protocoltests.restjson#RestJsonExtras",
        "rest_json_extras",
        dependsOn = listOf("rest-json-extras.smithy"),
    ),
    ClientTest("aws.protocoltests.misc#MiscService", "misc", dependsOn = listOf("misc.smithy")),
    ClientTest("aws.protocoltests.restxml#RestXml", "rest_xml", addMessageToErrors = false),
    ClientTest("aws.protocoltests.query#AwsQuery", "aws_query", addMessageToErrors = false),
    ClientTest("aws.protocoltests.ec2#AwsEc2", "ec2_query", addMessageToErrors = false),
    ClientTest("aws.protocoltests.rpcv2cbor#QueryCompatibleRpcV2Protocol", "rpcv2cbor_query_compatible"),
    ClientTest("aws.protocoltests.rpcv2cbor#NonQueryCompatibleRpcV2Protocol", "rpcv2cbor_non_query_compatible"),
    ClientTest("smithy.protocoltests.rpcv2Cbor#RpcV2Protocol", "rpcv2Cbor"),
    ClientTest(
        "smithy.protocoltests.rpcv2Cbor#RpcV2CborService",
        "rpcv2Cbor_extras",
        dependsOn = listOf("rpcv2Cbor-extras.smithy")
    ),
    ClientTest(
        "aws.protocoltests.restxml.xmlns#RestXmlWithNamespace",
        "rest_xml_namespace",
        addMessageToErrors = false,
    ),
    ClientTest("aws.protocoltests.restxml#RestXmlExtras", "rest_xml_extras", addMessageToErrors = false),
    ClientTest(
        "aws.protocoltests.restxmlunwrapped#RestXmlExtrasUnwrappedErrors",
        "rest_xml_extras_unwrapped",
        addMessageToErrors = false,
    ),
    ClientTest(
        "crate#Config",
        "naming_test_ops",
        dependsOn = listOf("naming-obstacle-course-ops.smithy"),
        renameErrors = false,
    ),
    ClientTest(
        "casing#ACRONYMInside_Service",
        "naming_test_casing",
        dependsOn = listOf("naming-obstacle-course-casing.smithy"),
    ),
    ClientTest(
        "naming_obs_structs#NamingObstacleCourseStructs",
        "naming_test_structs",
        dependsOn = listOf("naming-obstacle-course-structs.smithy"),
        renameErrors = false,
    ),
    ClientTest("aws.protocoltests.json#TestService", "endpoint-rules"),
    ClientTest(
        "com.aws.example#PokemonService",
        "pokemon-service-client",
        dependsOn = listOf("pokemon.smithy", "pokemon-common.smithy"),
    ),
    ClientTest(
        "com.aws.example#PokemonService",
        "pokemon-service-awsjson-client",
        dependsOn = listOf("pokemon-awsjson.smithy", "pokemon-common.smithy"),
    ),
    ClientTest("aws.protocoltests.misc#QueryCompatService", "query-compat-test", dependsOn = listOf("aws-json-query-compat.smithy")),
).map(ClientTest::toCodegenTest)

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)
project.registerCopyCheckedInCargoLockfileTask(checkedInSmithyRuntimeLockfile, layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)

tasks["generateSmithyBuild"].inputs.property("smithy.runtime.mode", getSmithyRuntimeMode())

tasks["smithyBuild"].dependsOn("generateSmithyBuild")
tasks["assemble"].finalizedBy("generateCargoWorkspace", "copyCheckedInCargoLockfile")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }
