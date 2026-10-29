/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.gradle.api.Project

/** Include compatibility SDKs only for a manual task, an explicit property, or selected modules. */
fun Project.optionalServerCompatibilityTests(tests: List<CodegenTest>): List<CodegenTest> {
    val properties = PropertyRetriever(rootProject, this)
    val manualTasks =
        setOf("serverCompatibilityTest", "generateServerCompatibilitySdk", "protocolCompatibilityTest", "compatibilityProtocolTest")
    val taskRequested = gradle.startParameter.taskNames.any { it.substringAfterLast(":") in manualTasks }
    val modules = properties.get("modules")?.split(",")?.map { it.trim() }.orEmpty()
    val moduleRequested = tests.any { it.module in modules }
    return if (properties.get("serverCompatibility")?.toBoolean() == true || taskRequested || moduleRequested) {
        tests
    } else {
        emptyList()
    }
}
