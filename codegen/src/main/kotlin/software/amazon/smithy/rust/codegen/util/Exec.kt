/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.util

import software.amazon.smithy.rust.codegen.smithy.letIf
import java.nio.file.Path
import java.util.concurrent.TimeUnit

class CommandFailed(output: String) : Exception("Command Failed\n$output")

fun String.runCommand(workdir: Path? = null, timeoutSecs: Long = 1000): String {
    val parts = this.split("\\s".toRegex())
    val proc = ProcessBuilder(*parts.toTypedArray())
        .redirectOutput(ProcessBuilder.Redirect.PIPE)
        .redirectError(ProcessBuilder.Redirect.PIPE)
        .letIf(workdir != null) {
            it.directory(workdir?.toFile())
        }
        .start()

    proc.waitFor(timeoutSecs, TimeUnit.SECONDS)
    val stdErr = proc.errorStream.bufferedReader().readText()
    val stdOut = proc.inputStream.bufferedReader().readText()
    val output = "$stdErr\n$stdOut"
    return when (proc.exitValue()) {
        0 -> output
        else -> throw CommandFailed("Command Failed\n$output")
    }
}
