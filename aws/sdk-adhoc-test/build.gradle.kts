/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

extra["displayName"] = "Smithy :: Rust :: AWS-SDK :: Ad-hoc Test"
extra["moduleName"] = "software.amazon.smithy.rust.awssdk.adhoc.test"

tasks["jar"].enabled = false

plugins {
    java
    alias(libs.plugins.smithy.gradle.base)
    alias(libs.plugins.smithy.gradle.jar)
}

val properties = PropertyRetriever(rootProject, project)

val pluginName = "rust-client-codegen"
val workingDirUnderBuildDir = "smithyprojections/sdk-adhoc-test/"

configure<software.amazon.smithy.gradle.SmithyExtension> {
    outputDirectory = layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile
}

val checkedInSdkLockfile = rootProject.projectDir.resolve("aws/sdk/Cargo.lock")

dependencies {
    implementation(project(":aws:sdk-codegen"))
    implementation(libs.smithy.aws.protocol.tests)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.model)
}

fun getNullabilityCheckMode(): String = properties.get("nullability.check.mode") ?: "CLIENT_CAREFUL"

fun baseTest(service: String, module: String, imports: List<String> = listOf()): CodegenTest {
    return CodegenTest(
        service = service,
        module = module,
        imports = imports,
        extraCodegenConfig = """
            "includeFluentClient": false,
            "nullabilityCheckMode": "${getNullabilityCheckMode()}"
        """,
    )
}

val allCodegenTests = listOf(
    baseTest(
        "com.amazonaws.testservice#TestService",
        "endpoint-test-service",
        imports = listOf("models/single-static-endpoint.smithy"),
    ),
    baseTest(
        "com.amazonaws.testservice#RequiredValues",
        "required-values",
        imports = listOf("models/required-value-test.smithy"),
    ),
    // service specific protocol tests
    baseTest(
        "com.amazonaws.apigateway#BackplaneControlService",
        "apigateway",
        imports = listOf("models/apigateway-rules.smithy"),
    ),
    baseTest(
        "com.amazonaws.glacier#Glacier",
        "glacier",
    ),
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/139) - we assume this will be handled by EP2.0 rules but
    //  the machinelearning service model has yet to be updated to include rules that handle the expected customization
    // baseTest(
    //     "com.amazonaws.machinelearning#AmazonML_20141212",
    //     "machinelearning",
    // ),
)

project.registerGenerateSmithyBuildTask(rootProject, pluginName, allCodegenTests)
project.registerGenerateCargoWorkspaceTask(rootProject, pluginName, allCodegenTests, workingDirUnderBuildDir)
project.registerGenerateCargoConfigTomlTask(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)
project.registerCopyCheckedInCargoLockfileTask(checkedInSdkLockfile, layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)

tasks["smithyBuild"].dependsOn("generateSmithyBuild")
tasks["assemble"].dependsOn("generateCargoWorkspace").finalizedBy("copyCheckedInCargoLockfile")

project.registerModifyMtimeTask()
project.registerCargoCommandsTasks(layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile)

tasks["test"].finalizedBy(cargoCommands(properties).map { it.toString })

tasks["clean"].doFirst { delete("smithy-build.json") }
