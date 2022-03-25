/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
import org.gradle.api.DefaultTask
import org.gradle.api.model.ObjectFactory
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputDirectory
import org.gradle.api.tasks.TaskAction
import org.gradle.process.internal.DefaultExecSpec
import org.gradle.process.internal.ExecActionFactory
import java.io.File
import javax.inject.Inject

/**
 * Runs a smithy-rs build tool that is written in Rust and also included in the Docker build base image.
 * If it detects that Gradle is running inside the Docker build image, it uses the prebuilt tool binary.
 * Otherwise, it compiles the tool and runs it.
 */
abstract class ExecRustBuildTool : DefaultTask() {
    @get:InputDirectory
    var toolPath: File? = null
    @get:Input
    var binaryName: String? = null
    @get:Input
    var arguments: List<String>? = null

    @Inject
    protected open fun getObjectFactory(): ObjectFactory = throw UnsupportedOperationException()
    @Inject
    protected open fun getExecActionFactory(): ExecActionFactory = throw UnsupportedOperationException()

    @TaskAction
    fun run() {
        checkNotNull(toolPath) { "toolPath must be set" }
        checkNotNull(binaryName) { "binaryName must be set" }
        checkNotNull(arguments) { "arguments must be set" }

        // When building with the build Docker image, the Rust tools are already on the path. Just use them.
        if (System.getenv()["SMITHY_RS_DOCKER_BUILD_IMAGE"] == "1") {
            runCli(listOf(binaryName!!) + arguments!!, workingDirectory = null)
        } else {
            runCli(
                listOf("cargo", "run", "--bin", binaryName!!, "--") + arguments!!,
                workingDirectory = toolPath
            )
        }
    }

    private fun runCli(commandLine: List<String>, workingDirectory: File? = null) {
        getExecActionFactory().newExecAction().let { action ->
            getObjectFactory()
                .newInstance(DefaultExecSpec::class.java)
                .apply {
                    commandLine(commandLine)
                    if (workingDirectory != null) {
                        workingDir(workingDirectory)
                    }
                }
                .copyTo(action)
            action.execute()
        }
    }
}
