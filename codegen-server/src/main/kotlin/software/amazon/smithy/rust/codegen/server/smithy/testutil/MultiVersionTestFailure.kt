/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.testutil

import kotlin.reflect.KClass

/**
 * Exception thrown when tests fail for one or more HTTP versions in dual-version testing.
 *
 * This exception provides structured access to failures across multiple HTTP versions,
 * allowing tests to programmatically inspect which versions failed and with what exceptions.
 */
class MultiVersionTestFailure(
    val failures: List<HttpVersionFailure>,
) : AssertionError(buildMessage(failures), failures.firstOrNull()?.exception) {
    init {
        // Add remaining exceptions as suppressed for compatibility with existing error reporting
        failures.drop(1).forEach { addSuppressed(it.exception) }
    }

    /**
     * Represents a test failure for a specific HTTP version.
     */
    data class HttpVersionFailure(
        val version: HttpTestVersion,
        val exception: Throwable,
    )

    /**
     * Returns true if all failures are of the specified exception type.
     */
    fun allFailuresAreOfType(type: KClass<out Throwable>): Boolean = failures.all { type.isInstance(it.exception) }

    /**
     * Returns failures that are of the specified exception type.
     */
    fun getFailuresOfType(type: KClass<out Throwable>): List<HttpVersionFailure> =
        failures.filter { type.isInstance(it.exception) }

    /**
     * Returns true if there is a failure for the specified HTTP version.
     */
    fun hasFailureFor(version: HttpTestVersion): Boolean = failures.any { it.version == version }

    /**
     * Returns the failure for the specified HTTP version, or null if none exists.
     */
    fun getFailureFor(version: HttpTestVersion): HttpVersionFailure? = failures.find { it.version == version }

    companion object {
        private fun buildMessage(failures: List<HttpVersionFailure>): String =
            buildString {
                appendLine("Test failed for ${failures.size} HTTP version(s):")
                failures.forEach { (version, exception) ->
                    appendLine()
                    appendLine("=== Failure in ${version.displayName} ===")
                    appendLine(exception.message ?: exception.toString())
                }
            }
    }
}
