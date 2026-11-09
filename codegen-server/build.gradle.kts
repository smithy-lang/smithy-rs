/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

plugins {
    id("smithy-rs.kotlin-conventions")
    id("smithy-rs.publishing-conventions")
}

description = "Generates Rust server-side code from Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Server"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.server"

dependencies {
    implementation(project(":codegen-core"))
    implementation(project(":codegen-traits"))
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.protocol.traits)

    // `smithy.framework#ValidationException` is defined here, which is used in `constraints.smithy`, which is used
    // in `CustomValidationExceptionWithReasonDecoratorTest`.
    testImplementation(libs.smithy.validation.model)

    // It's handy to re-use protocol test suite models from Smithy in our Kotlin tests.
    testImplementation(libs.smithy.protocol.tests)

    testImplementation(libs.junit.jupiter)
    testImplementation(libs.kotest.assertions.core.jvm)
}

// Embed the artifact version so generated schema crates can record it separately from the source commit.
val generatedResourcesDir = layout.buildDirectory.dir("generated/resources")
val generateServerCodegenVersion by tasks.registering {
    val codegenVersion = project.version.toString()
    inputs.property("codegenVersion", codegenVersion)
    outputs.dir(generatedResourcesDir)
    doLast {
        val versionFile =
            generatedResourcesDir.get()
                .file("software/amazon/smithy/rust/codegen/server/smithy/codegen-release.txt").asFile
        versionFile.parentFile.mkdirs()
        versionFile.writeText(codegenVersion)
    }
}

sourceSets.main {
    resources.srcDir(generatedResourcesDir)
}

tasks.processResources {
    dependsOn(generateServerCodegenVersion)
}

tasks.named("sourcesJar").configure {
    dependsOn(generateServerCodegenVersion)
}

// CI keeps focused codegen regressions; full legacy/schema comparisons run separately.
tasks.test {
    if (providers.gradleProperty("excludeServerCompatibilityTests").map { it.toBoolean() }.getOrElse(false)) {
        useJUnitPlatform {
            excludeTags("server-compatibility")
        }
    }

    // `-PtestShard=i/n` runs only the test classes in shard `i` of `n` (1-based), so CI can split the
    // suite across jobs. The slowest classes are spread first, each to the least-loaded shard; every
    // other class goes to a shard chosen by a hash of its name. Both are stable across runs and
    // machines, and nested and lambda classes (`Outer$...`) stay with their top-level class.
    providers.gradleProperty("testShard").orNull?.let { shard ->
        val parts = shard.split("/").map { it.trim().toIntOrNull() }
        val index = parts.getOrNull(0)
        val count = parts.getOrNull(1)
        require(parts.size == 2 && index != null && count != null && count > 0 && index in 1..count) {
            "testShard must be `i/n` with 1 <= i <= n, got `$shard`"
        }
        // Approximate seconds per class, from a local run. Hashing alone put six of these in one of four
        // shards. Update when a class becomes much slower or faster; a stale entry only unbalances shards.
        val slowClasses =
            mapOf(
                "Http1xDependencyTest" to 507,
                "ServerBuilderDefaultValuesTest" to 308,
                "ServerEventStreamMarshallerGeneratorTest" to 232,
                "RecursiveConstraintViolationsTest" to 197,
                "ConstrainedNumberGeneratorTest" to 130,
                "ConstraintsTest" to 105,
                "ServerHttpSensitivityGeneratorTest" to 103,
                "NamingObstacleCourseTest" to 99,
                "ServerEventStreamUnmarshallerGeneratorTest" to 94,
                "RequestBodyMaxBytesTest" to 86,
            )
        val load = IntArray(count)
        val slowClassShards =
            slowClasses.entries.sortedWith(compareByDescending<Map.Entry<String, Int>> { it.value }.thenBy { it.key })
                .associate { (name, seconds) ->
                    val target = load.indices.minBy { load[it] }
                    load[target] += seconds
                    name to target
                }
        exclude { element ->
            val path = element.relativePath.pathString
            if (element.isDirectory || !path.endsWith(".class")) {
                false
            } else {
                val topLevelClass = path.removeSuffix(".class").substringBefore('$')
                val target =
                    slowClassShards[topLevelClass.substringAfterLast('/')]
                        ?: Math.floorMod(topLevelClass.hashCode(), count)
                target != index - 1
            }
        }
    }
}

val compatibilityTest by tasks.registering(Test::class) {
    group = "verification"
    description = "Runs the server legacy/schema compatibility comparisons excluded from CI."
    testClassesDirs = sourceSets.test.get().output.classesDirs
    classpath = sourceSets.test.get().runtimeClasspath
    useJUnitPlatform {
        includeTags("server-compatibility")
    }
}
