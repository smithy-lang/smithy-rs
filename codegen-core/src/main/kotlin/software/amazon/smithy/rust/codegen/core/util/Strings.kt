/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import software.amazon.smithy.utils.CaseUtils
import software.amazon.smithy.utils.StringUtils

fun String.doubleQuote(): String =
    StringUtils.escapeJavaString(this, "").replace(Regex("""\\u([0-9a-f]{4})""")) { matchResult: MatchResult ->
        "\\u{" + matchResult.groupValues[1] + "}" as CharSequence
    }

/**
 * Double quote a string, e.g. "abc" -> "\"abc\""
 */
fun String.dq(): String = this.doubleQuote()

private val completeWords: List<String> = listOf("ipv4", "ipv6", "sigv4", "mib", "gib", "kib", "ttl")

private fun String.splitOnWordBoundaries(): List<String> {
    val out = mutableListOf<String>()
    // These are whole words but cased differently, e.g. `IPv4`, `MiB`, `GiB`, `TtL`
    var currentWord = ""

    var completeWordInProgress = true
    // emit the current word and update from the next character
    val emit = { next: Char ->
        completeWordInProgress = true
        if (currentWord.isNotEmpty()) {
            out += currentWord.lowercase()
        }
        currentWord = if (next.isLetterOrDigit()) {
            next.toString()
        } else {
            ""
        }
    }
    val allLowerCase = this.lowercase() == this
    this.forEachIndexed { index, nextCharacter ->
        val computeWordInProgress = {
            val result = completeWordInProgress && currentWord.isNotEmpty() && completeWords.any {
                it.startsWith(currentWord, ignoreCase = true) && (currentWord + this.substring(index)).startsWith(
                    it,
                    ignoreCase = true,
                ) && !it.equals(currentWord, ignoreCase = true)
            }

            completeWordInProgress = result
            result
        }
        when {
            // [C] in these docs indicates the value of nextCharacter
            // A[_]B
            !nextCharacter.isLetterOrDigit() -> emit(nextCharacter)

            // If we have no letters so far, push the next letter (we already know it's a letter or digit)
            currentWord.isEmpty() -> currentWord += nextCharacter.toString()

            // Abc[D]ef or Ab2[D]ef
            !computeWordInProgress() && loweredFollowedByUpper(currentWord, nextCharacter) -> emit(nextCharacter)

            // s3[k]ey
            !computeWordInProgress() && allLowerCase && digitFollowedByLower(currentWord, nextCharacter) -> emit(
                nextCharacter,
            )

            // DB[P]roxy, or `IAM[U]ser` but not AC[L]s
            endOfAcronym(currentWord, nextCharacter, this.getOrNull(index + 1), this.getOrNull(index + 2)) -> emit(nextCharacter)

            // If we haven't found a word boundary, push it and keep going
            else -> currentWord += nextCharacter.toString()
        }
    }
    if (currentWord.isNotEmpty()) {
        out += currentWord
    }
    return out
}

/**
 * Handle cases like `DB[P]roxy`, `ARN[S]upport`, `AC[L]s`
 */
private fun endOfAcronym(current: String, nextChar: Char, peek: Char?, doublePeek: Char?): Boolean {
    if (!current.last().isUpperCase()) {
        // Not an acronym in progress
        return false
    }
    if (!nextChar.isUpperCase()) {
        // We aren't at the next word yet
        return false
    }

    if (peek?.isLowerCase() != true) {
        return false
    }

    // Skip cases like `AR[N]s`, `AC[L]s` but not `IAM[U]ser`
    if (peek == 's' && (doublePeek == null || !doublePeek.isLowerCase())) {
        return false
    }

    // Skip cases like `DynamoD[B]v2`
    if (peek == 'v' && doublePeek?.isDigit() == true) {
        return false
    }
    return true
}

private fun loweredFollowedByUpper(current: String, nextChar: Char): Boolean {
    if (!nextChar.isUpperCase()) {
        return false
    }
    return current.last().isLowerCase() || current.last().isDigit()
}

private fun digitFollowedByLower(current: String, nextChar: Char): Boolean {
    return (current.last().isDigit() && nextChar.isLowerCase())
}

fun String.toSnakeCase(): String {
    return this.splitOnWordBoundaries().joinToString("_") { it.lowercase() }
}

fun String.toPascalCase(): String {
    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3047): consider using our updated toSnakeCase (but need to audit diff)
    return CaseUtils.toSnakeCase(this).let { CaseUtils.toPascalCase(it) }
}
