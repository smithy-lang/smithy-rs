/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

description = "Measures JaCoCo line coverage of the server codegen (Kotlin) across a corpus of Smithy models"
extra["displayName"] = "Smithy :: Rust :: Codegen :: Coverage"
extra["moduleName"] = "software.amazon.smithy.rust.codegen.coverage"

plugins {
    java
    jacoco
}

tasks["jar"].enabled = false

val serverPluginName = "rust-server-codegen"
val clientPluginName = "rust-client-codegen"
val workingDirUnderBuildDir = "smithyprojections/coverage/"

dependencies {
    implementation(project(":codegen-server"))
    implementation(project(":codegen-client"))
    implementation(libs.smithy.cli)
    implementation(libs.smithy.aws.protocol.tests)
    implementation(libs.smithy.protocol.tests)
    implementation(libs.smithy.protocol.test.traits)
    implementation(libs.smithy.aws.traits)
    implementation(libs.smithy.validation.model)
    implementation(libs.smithy.waiters)
}

jacoco {
    toolVersion = "0.8.12"
}

// The corpus of models exercised for coverage. Every .smithy file under models/ is added
// automatically as its own projection; the service shape is discovered by the codegen plugin
// only if there is exactly one service, so each model file must contain exactly one service
// annotated with `// coverage-service: <shape id>` on the first line, or default to scanning.
data class CoverageModel(
    val service: String,
    val module: String,
    val imports: List<String>,
    val extraCodegenConfig: String? = null,
    // any subset of ["server", "client"]
    val plugins: List<String> = listOf("server", "client"),
    // raw JSON for the projection's "transforms" array
    val transforms: String? = null,
)

// Parse directives from the leading comment lines of each model file under models/:
//   // coverage-service: <shape id>          (required)
//   // coverage-import: <path>               (resolved against codegen-core/common-test-models, then repo root)
//   // coverage-codegen: <json fragment>     (spliced into the "codegen" settings object)
//   // coverage-plugins: server,client       (default: both)
//   // coverage-transforms: [{...}]           (raw JSON projection transforms array)
fun discoverModels(): List<CoverageModel> {
    val commonModels = rootProject.projectDir.resolve("codegen-core/common-test-models")
    val modelsDir = projectDir.resolve("models")
    val models = mutableListOf<CoverageModel>()
    modelsDir.listFiles { f -> f.extension == "smithy" }?.sorted()?.forEach { file ->
        var service: String? = null
        var extraCodegen: String? = null
        var transforms: String? = null
        var plugins = listOf("server", "client")
        val imports = mutableListOf(file.absolutePath)
        file.useLines { lines ->
            for (line in lines) {
                val l = line.trim()
                when {
                    l.startsWith("// coverage-service:") -> service = l.removePrefix("// coverage-service:").trim()
                    l.startsWith("// coverage-codegen:") -> extraCodegen = l.removePrefix("// coverage-codegen:").trim()
                    l.startsWith("// coverage-transforms:") -> transforms = l.removePrefix("// coverage-transforms:").trim()
                    l.startsWith("// coverage-plugins:") ->
                        plugins = l.removePrefix("// coverage-plugins:").split(",").map { it.trim() }
                    l.startsWith("// coverage-import:") -> {
                        val rel = l.removePrefix("// coverage-import:").trim()
                        val resolved = commonModels.resolve(rel).takeIf { it.exists() } ?: rootProject.projectDir.resolve(rel)
                        require(resolved.exists()) { "coverage-import `$rel` in ${file.name} not found" }
                        imports.add(resolved.absolutePath)
                    }
                    l.startsWith("//") || l.isEmpty() || l.startsWith("$") -> {}
                    else -> return@useLines
                }
            }
        }
        requireNotNull(service) { "Model ${file.name} is missing a `// coverage-service: <shape id>` directive" }
        models.add(
            CoverageModel(
                service = service!!,
                module = file.nameWithoutExtension,
                imports = imports,
                extraCodegenConfig = extraCodegen,
                plugins = plugins,
                transforms = transforms,
            ),
        )
    }
    require(models.isNotEmpty()) { "No models found under ${modelsDir.absolutePath}" }
    return models
}

fun generateSmithyBuildJson(models: List<CoverageModel>): String {
    fun pluginConfig(
        m: CoverageModel,
        plugin: String,
    ) = """
        "$plugin": {
            "runtimeConfig": {
                "relativePath": "${rootProject.projectDir.resolve("rust-runtime").invariantSeparatorsPath}"
            },
            "codegen": {
                ${m.extraCodegenConfig ?: ""}
            },
            "service": "${m.service}",
            "module": "${toRustCrateName(m.module)}",
            "moduleVersion": "0.0.1",
            "moduleDescription": "coverage",
            "moduleAuthors": ["coverage@example.com"]
        }
        """.trimIndent()
    // Server-only corpus, each model generated twice: the legacy serde path and the
    // schema-serde path (`"schemaSerde": true`). Client-only models are skipped.
    val projections =
        models.filter { it.plugins.contains("server") }.flatMap { m ->
            val imports = m.imports.joinToString(", ") { "\"${File(it).invariantSeparatorsPath}\"" }
            val transforms = m.transforms?.let { """"transforms": $it,""" } ?: ""
            val schemaConfig = listOfNotNull(""""schemaSerde": true""", m.extraCodegenConfig).joinToString(", ")
            listOf(
                """
                "${m.module}": {
                    "imports": [$imports],
                    $transforms
                    "plugins": {
                        ${pluginConfig(m, serverPluginName)}
                    }
                }
                """.trimIndent(),
                """
                "${m.module}_schema": {
                    "imports": [$imports],
                    $transforms
                    "plugins": {
                        ${pluginConfig(m.copy(extraCodegenConfig = schemaConfig), serverPluginName)}
                    }
                }
                """.trimIndent(),
            )
        }.joinToString(",\n")
    return """
        {
            "version": "1.0",
            "projections": {
                $projections
            }
        }
        """.trimIndent()
}

val generateSmithyBuild =
    tasks.register("generateSmithyBuild") {
        description = "Generate smithy-build.json for the coverage corpus"
        inputs.dir(projectDir.resolve("models"))
        outputs.file(layout.buildDirectory.file("smithy-build.json").get().asFile)
        doFirst {
            val configFile = layout.buildDirectory.file("smithy-build.json").get().asFile
            configFile.parentFile.mkdirs()
            configFile.writeText(generateSmithyBuildJson(discoverModels()))
        }
    }

val jacocoExecFile = layout.buildDirectory.file("jacoco/codegenCoverage.exec")

val runCodegenCoverage =
    tasks.register<JavaExec>("runCodegenCoverage") {
        description = "Run server codegen over the coverage corpus with the JaCoCo agent attached"
        dependsOn(generateSmithyBuild)
        outputs.upToDateWhen { false }
        classpath = sourceSets["main"].runtimeClasspath
        mainClass.set("software.amazon.smithy.cli.SmithyCli")
        args(
            "build",
            "--config", layout.buildDirectory.file("smithy-build.json").get().asFile.absolutePath,
            "--output", layout.buildDirectory.dir(workingDirUnderBuildDir).get().asFile.absolutePath,
            "--discover",
        )
        maxHeapSize = "4g"
        project.extensions.getByType<JacocoPluginExtension>().applyTo(this)
        configure<JacocoTaskExtension> {
            setDestinationFile(jacocoExecFile.get().asFile)
        }
        doFirst {
            jacocoExecFile.get().asFile.delete()
        }
    }

val coverageProjects = listOf(":codegen-core", ":codegen-server", ":codegen-client")

// Code that no model can reach:
// - testutil: test-only infrastructure, never exercised by a production codegen run
// - SchemaGenerator/SchemaTraitFilter: schema-based serde is gated behind
//   `SchemaSerdeAllowlist`, which is intentionally hardcoded to empty (no setting can enable it)
val coverageExclusions =
    listOf(
        "**/testutil/**",
        "**/generators/SchemaGenerator*",
        "**/generators/SchemaTraitFilter*",
        // Only instantiated by the AWS SDK codegen plugin (aws/codegen-aws-sdk), which is out of
        // scope for the generic rust-client-codegen / rust-server-codegen plugins measured here
        "**/client/smithy/customize/ConditionalDecorator*",
        "**/client/smithy/customizations/DocsRsMetadataDecorator*",
        // Only enabled via a META-INF/services resource file, not reachable through any model
        // or codegen setting
        "**/server/smithy/customizations/AdditionalErrorsDecorator*",
    )

val codegenCoverageReport =
    tasks.register<JacocoReport>("codegenCoverageReport") {
        description = "Generate the JaCoCo report for the codegen coverage run"
        dependsOn(runCodegenCoverage)
        executionData(jacocoExecFile.get().asFile)
        classDirectories.setFrom(
            coverageProjects.map { p ->
                fileTree(project(p).layout.buildDirectory.dir("classes/kotlin/main")) {
                    exclude(coverageExclusions)
                }
            },
        )
        sourceDirectories.setFrom(
            coverageProjects.map { p ->
                project(p).projectDir.resolve("src/main/kotlin")
            },
        )
        reports {
            xml.required.set(true)
            html.required.set(true)
            xml.outputLocation.set(layout.buildDirectory.file("reports/jacoco/coverage.xml"))
            html.outputLocation.set(layout.buildDirectory.dir("reports/jacoco/html"))
        }
    }

tasks.register("coverage") {
    description = "Run codegen over the corpus and produce the JaCoCo coverage report"
    dependsOn(codegenCoverageReport)
}
