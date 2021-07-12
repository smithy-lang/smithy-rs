/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.aws.traits.ServiceTrait
import kotlin.streams.toList

extra["displayName"] = "Smithy :: Rust :: AWS-SDK"
extra["moduleName"] = "software.amazon.smithy.rust.awssdk"

tasks["jar"].enabled = false

plugins {
    id("software.amazon.smithy").version("0.5.3")
}

val smithyVersion: String by project

val sdkOutputDir = buildDir.resolve("aws-sdk")
val runtimeModules = listOf(
    "smithy-types",
    "smithy-json",
    "smithy-query",
    "smithy-xml",
    "smithy-http",
    "smithy-http-tower",
    "smithy-client",
    "protocol-test-helpers"
)
val awsModules = listOf("aws-auth", "aws-endpoint", "aws-types", "aws-hyper", "aws-sig-auth", "aws-http")

buildscript {
    val smithyVersion: String by project
    dependencies {
        classpath("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
        classpath("software.amazon.smithy:smithy-cli:$smithyVersion")
    }
}

dependencies {
    implementation(project(":aws:sdk-codegen"))
    implementation("software.amazon.smithy:smithy-protocol-test-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-iam-traits:$smithyVersion")
    implementation("software.amazon.smithy:smithy-aws-cloudformation-traits:$smithyVersion")
}

// Tier 1 Services have examples and tests
val tier1Services = setOf(
    "apigateway",
    "autoscaling",
    "batch",
    "cloudformation",
    "cloudwatch",
    "cloudwatchlogs",
    "cognitoidentity",
    "cognitoidentityprovider",
    "cognitosync",
    "config",
    "dynamodb",
    "ebs",
    "ec2",
    "ecr",
    "ecs",
    "eks",
    "iam",
    "kinesis",
    "kms",
    "lambda",
    "medialive",
    "mediapackage",
    "polly",
    "qldb",
    "qldbsession",
    "rds",
    "rdsdata",
    "route53",
    "s3",
    "sagemaker",
    "sagemakera2iruntime",
    "sagemakeredge",
    "sagemakerfeaturestoreruntime",
    "secretsmanager",
    "sesv2",
    "snowball",
    "sns",
    "sqs",
    "ssm",
    "sts"
)

private val disableServices = setOf("transcribestreaming")

data class AwsService(
    val service: String,
    val module: String,
    val modelFile: File,
    val extraConfig: String? = null,
    val extraFiles: List<File>
) {
    fun files(): List<File> = listOf(modelFile) + extraFiles
}

val generateAllServices =
    project.providers.environmentVariable("GENERATE_ALL_SERVICES").forUseAtConfigurationTime().orElse("")

val generateOnly: Provider<Set<String>> =
    project.providers.environmentVariable("GENERATE_ONLY")
        .forUseAtConfigurationTime()
        .map { envVar ->
            envVar.split(",").filter { service -> service.trim().isNotBlank() }
        }
        .orElse(listOf())
        .map { it.toSet() }

val awsServices: Provider<List<AwsService>> = generateAllServices.zip(generateOnly) { v, only ->
    discoverServices(v.toLowerCase() == "true", only)
}

/**
 * Discovers services from the `models` directory
 *
 * Do not invoke this function directly. Use the `awsServices` provider.
 */
fun discoverServices(allServices: Boolean, generateOnly: Set<String>): List<AwsService> {
    val models = project.file("aws-models")
    val services = fileTree(models)
        .sortedBy { file -> file.name }
        .mapNotNull { file ->
        val model = Model.assembler().addImport(file.absolutePath).assemble().result.get()
        val services: List<ServiceShape> = model.shapes(ServiceShape::class.java).sorted().toList()
        if (services.size > 1) {
            throw Exception("There must be exactly one service in each aws model file")
        }
        if (services.isEmpty()) {
            logger.info("${file.name} has no services")
            null
        } else {
            val service = services[0]
            val sdkId = service.expectTrait(ServiceTrait::class.java).sdkId
                .toLowerCase()
                .replace(" ", "")
                // TODO: the smithy models should not include the suffix "service"
                .removeSuffix("service")
                .removeSuffix("api")
            val testFile = file.parentFile.resolve("$sdkId-tests.smithy")
            val extras = if (testFile.exists()) {
                logger.warn("Discovered protocol tests for ${file.name}")
                listOf(testFile)
            } else {
                listOf()
            }
            AwsService(service = service.id.toString(), module = sdkId, modelFile = file, extraFiles = extras)
        }
    }.filterNot {
        disableServices.contains(it.module)
    }.filter {
        val inGenerateOnly = generateOnly.isNotEmpty() && generateOnly.contains(it.module)
        val inTier1 = generateOnly.isEmpty() && tier1Services.contains(it.module)
        allServices || inGenerateOnly || inTier1
    }
    if (generateOnly.isNotEmpty()) {
        val modules = services.map { it.module }.toSet()
        tier1Services.forEach { service ->
            check(modules.contains(service)) { "Service $service was in list of tier 1 services but not generated!" }
        }
    }
    return services
}

fun generateSmithyBuild(tests: List<AwsService>): String {
    val projections = tests.joinToString(",\n") {
        val files = it.files().map { extraFile ->
            software.amazon.smithy.utils.StringUtils.escapeJavaString(
                extraFile.absolutePath,
                ""
            )
        }
        """
            "${it.module}": {
                "imports": [${files.joinToString()}],

                "plugins": {
                    "rust-codegen": {
                      "runtimeConfig": {
                        "relativePath": "../"
                      },
                      "codegen": {
                        "includeFluentClient": false,
                        "renameErrors": false
                      },
                      "service": "${it.service}",
                      "module": "aws-sdk-${it.module}",
                      "moduleVersion": "0.0.11-alpha",
                      "moduleAuthors": ["AWS Rust SDK Team <aws-sdk-rust@amazon.com>", "Russell Cohen <rcoh@amazon.com>"],
                      "license": "Apache-2.0"
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
    dependsOn(awsServices)
    doFirst {
        projectDir.resolve("smithy-build.json").writeText(generateSmithyBuild(awsServices.get()))
    }
    inputs.dir(projectDir.resolve("aws-models"))
    outputs.file(projectDir.resolve("smithy-build.json"))
}

task("relocateServices") {
    description = "relocate AWS services to their final destination"
    doLast {
        awsServices.get().forEach {
            logger.info("Relocating ${it.module}...")
            copy {
                from("$buildDir/smithyprojections/sdk/${it.module}/rust-codegen")
                into(sdkOutputDir.resolve(it.module))
            }

            copy {
                from(projectDir.resolve("integration-tests/${it.module}/tests"))
                into(sdkOutputDir.resolve(it.module).resolve("tests"))
            }

            copy {
                from(projectDir.resolve("integration-tests/${it.module}/benches"))
                into(sdkOutputDir.resolve(it.module).resolve("benches"))
            }
        }
    }
    inputs.dir("$buildDir/smithyprojections/sdk/")
    outputs.dir(sdkOutputDir)
}

task("relocateExamples") {
    description = "relocate the examples folder & rewrite path dependencies"
    doLast {
        copy {
            from(projectDir)
            include("examples/**")
            into(sdkOutputDir)
            exclude("**/target")
            filter { line -> line.replace("build/aws-sdk/", "") }
        }
    }
    inputs.dir(projectDir.resolve("examples"))
    outputs.dir(sdkOutputDir)
}

tasks.register<Copy>("relocateAwsRuntime") {
    from("$rootDir/aws/rust-runtime")
    awsModules.forEach {
        include("$it/**")
    }
    exclude("**/target")
    exclude("**/Cargo.lock")
    filter { line -> rewritePathDependency(line) }
    into(sdkOutputDir)
}

/**
 * The aws/rust-runtime crates depend on local versions of the Smithy core runtime enabling local compilation. However,
 * those paths need to be replaced in the final build. We should probably fix this with some symlinking.
 */
fun rewritePathDependency(line: String): String {
    return line.replace("../../rust-runtime/", "")
}

tasks.register<Copy>("relocateRuntime") {
    from("$rootDir/rust-runtime") {
        runtimeModules.forEach {
            include("$it/**")
        }
        exclude("**/target")
        exclude("**/Cargo.lock")
    }
    into(sdkOutputDir)
}

fun generateCargoWorkspace(services: List<AwsService>): String {
    val generatedModules = services.map { it.module }.toSet()
    val examples = projectDir.resolve("examples")
        .listFiles { file -> !file.name.startsWith(".") }.orEmpty().toList()
        .filter { generatedModules.contains(it.name) }
        .map { "examples/${it.name}" }

    val modules = services.map(AwsService::module) + runtimeModules + awsModules + examples.toList()
    return """
    [workspace]
    members = [
        ${modules.joinToString(",") { "\"$it\"" }}
    ]
    """.trimIndent()
}
task("generateCargoWorkspace") {
    description = "generate Cargo.toml workspace file"
    doFirst {
        sdkOutputDir.mkdirs()
        sdkOutputDir.resolve("Cargo.toml").writeText(generateCargoWorkspace(awsServices.get()))
    }
    dependsOn(awsServices)
    inputs.dir(projectDir.resolve("examples"))
    outputs.file(sdkOutputDir.resolve("Cargo.toml"))
    outputs.upToDateWhen { false }
}

task("finalizeSdk") {
    dependsOn("assemble")
    outputs.upToDateWhen { false }
    finalizedBy(
        "relocateServices",
        "relocateRuntime",
        "relocateAwsRuntime",
        "relocateExamples"
    )
}

tasks["smithyBuildJar"].inputs.file(projectDir.resolve("smithy-build.json"))
tasks["smithyBuildJar"].inputs.dir(projectDir.resolve("aws-models"))
tasks["smithyBuildJar"].dependsOn("generateSmithyBuild")
tasks["smithyBuildJar"].dependsOn(awsServices)
tasks["smithyBuildJar"].dependsOn("generateCargoWorkspace")
tasks["smithyBuildJar"].outputs.upToDateWhen { false }
tasks["assemble"].dependsOn("smithyBuildJar")
tasks["assemble"].finalizedBy("finalizeSdk")

tasks.register<Exec>("cargoCheck") {
    workingDir(sdkOutputDir)
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "check", "--lib", "--tests", "--benches")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoTest") {
    workingDir(sdkOutputDir)
    // disallow warnings
    environment("RUSTFLAGS", "-D warnings")
    commandLine("cargo", "test")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoDocs") {
    workingDir(sdkOutputDir)
    // disallow warnings
    environment("RUSTDOCFLAGS", "-D warnings")
    commandLine("cargo", "doc", "--no-deps", "--document-private-items")
    dependsOn("assemble")
}

tasks.register<Exec>("cargoClippy") {
    workingDir(sdkOutputDir)
    // disallow warnings
    commandLine("cargo", "clippy", "--", "-D", "warnings")
    dependsOn("assemble")
}

tasks.register<RunExampleTask>("runExample") {
    dependsOn("assemble")
    outputDir = sdkOutputDir
}

// TODO: validate that the example exists. Otherwise this fails with a hiden error.
open class RunExampleTask @javax.inject.Inject constructor() : Exec() {
    @Option(option = "example", description = "Example to run")
    var example: String? = null
        set(value) {
            workingDir = workingDir.resolve(value!!)
            field = value
        }

    @org.gradle.api.tasks.InputDirectory
    var outputDir: File? = null
        set(value) {
            workingDir = value!!.resolve("examples")
            commandLine = listOf("cargo", "run")
            field = value
        }
}

tasks["test"].finalizedBy("cargoClippy", "cargoTest", "cargoDocs")

tasks["clean"].doFirst {
    delete("smithy-build.json")
}
