/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import java.io.IOException
import java.nio.file.Path
import java.util.concurrent.TimeUnit
import java.util.logging.Logger

data class CommandError(val output: String) : Exception("Command Error\n$output")

fun String.runCommand(
    workdir: Path? = null,
    environment: Map<String, String> = mapOf(),
    timeout: Long = 3600,
    redirect: ProcessBuilder.Redirect = ProcessBuilder.Redirect.PIPE,
): String {
    val logger = Logger.getLogger("RunCommand")
    logger.fine("Invoking comment $this in `$workdir` with env $environment")
    val start = System.currentTimeMillis()
    val parts = this.split("\\s".toRegex())
    val builder =
        ProcessBuilder(*parts.toTypedArray())
            .redirectOutput(redirect)
            .redirectError(redirect)
            .letIf(workdir != null) {
                it.directory(workdir?.toFile())
            }

    val env = builder.environment()
    environment.forEach { (k, v) -> env[k] = v }
    try {
        val proc = builder.start()
        proc.waitFor(timeout, TimeUnit.SECONDS)
        val stdErr = proc.errorStream.bufferedReader().readText()
        val stdOut = proc.inputStream.bufferedReader().readText()
        val output = "$stdErr\n$stdOut"
        return when (proc.exitValue()) {
            0 -> output
            else -> throw CommandError("Command Error\n$output")
        }
    } catch (_: IllegalThreadStateException) {
        throw CommandError("Timeout")
    } catch (err: IOException) {
        throw CommandError("$this was not a valid command.\n  Hint: is everything installed?\n$err")
    } catch (other: Exception) {
        throw CommandError("Unexpected exception thrown when executing subprocess:\n$other")
    } finally {
        val end = System.currentTimeMillis()
        logger.fine("command duration: ${end - start}ms")
    }
}
