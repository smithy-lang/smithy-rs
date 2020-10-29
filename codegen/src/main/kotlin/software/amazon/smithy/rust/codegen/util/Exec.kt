/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.util

import java.nio.file.Path
import java.util.concurrent.TimeUnit
import software.amazon.smithy.rust.codegen.smithy.letIf

class CommandFailed(output: String) : Exception("Command Failed\n$output")

fun String.runCommand(workdir: Path? = null): String? {
    val parts = this.split("\\s".toRegex())
    val proc = ProcessBuilder(*parts.toTypedArray())
        .redirectOutput(ProcessBuilder.Redirect.PIPE)
        .redirectError(ProcessBuilder.Redirect.PIPE)
        .letIf(workdir != null) {
            it.directory(workdir?.toFile())
        }
        .start()

    proc.waitFor(60, TimeUnit.MINUTES)
    if (proc.exitValue() != 0) {
        val stdErr = proc.errorStream.bufferedReader().readText()
        val stdOut = proc.inputStream.bufferedReader().readText()
        throw CommandFailed("Command Failed\n$stdErr\n$stdOut")
    }
    return proc.inputStream.bufferedReader().readText()
}
