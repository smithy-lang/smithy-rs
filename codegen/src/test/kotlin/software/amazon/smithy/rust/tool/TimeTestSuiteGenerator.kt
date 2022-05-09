/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.tool

import software.amazon.smithy.model.SourceLocation
import software.amazon.smithy.model.node.ArrayNode
import software.amazon.smithy.model.node.BooleanNode
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.NumberNode
import software.amazon.smithy.model.node.ObjectNode
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeFormatterBuilder
import java.time.format.SignStyle
import java.time.temporal.ChronoField
import java.util.Locale
import kotlin.math.absoluteValue

private val UTC = ZoneId.of("UTC")
private val YEARS = listOf(-9999, -100, -1, /* year 0 doesn't exist */ 1, 100, 1969, 1970, 2037, 2038, 9999)
private val DAYS_IN_MONTH = mapOf(
    1 to 31,
    2 to 28,
    3 to 31,
    4 to 30,
    5 to 31,
    6 to 30,
    7 to 31,
    8 to 31,
    9 to 30,
    10 to 31,
    11 to 30,
    12 to 31
)
private val MILLI_FRACTIONS = listOf(0, 1_000_000, 10_000_000, 100_000_000, 234_000_000)
private val MICRO_FRACTIONS = listOf(0, 1_000, 10_000, 100_000, 234_000)
private val NANO_FRACTIONS =
    listOf(0, 1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 123_456_789)

private data class TestCase(
    val time: ZonedDateTime,
    val formatted: String?,
) {
    fun toNode(): Node =
        time.toInstant().let { instant ->
            val map = mutableMapOf<String, Node>(
                "iso8601" to Node.from(DateTimeFormatter.ISO_OFFSET_DATE_TIME.format(time)),
                // JSON numbers have 52 bits of precision, and canonical seconds needs 64 bits
                "canonical_seconds" to Node.from(instant.epochSecond.toString()),
                "canonical_nanos" to NumberNode(instant.nano, SourceLocation.NONE),
                "error" to BooleanNode(formatted == null, SourceLocation.NONE)
            )
            if (formatted != null) {
                map["smithy_format_value"] = Node.from(formatted)
            }
            return ObjectNode(map.mapKeys { Node.from(it.key) }, SourceLocation.NONE)
        }
}

private enum class AllowedSubseconds {
    NANOS,
    MICROS,
    MILLIS,
}

private fun generateTestTimes(allowed: AllowedSubseconds): List<ZonedDateTime> {
    val result = ArrayList<ZonedDateTime>()
    var i = 887 // a prime number to start
    for (year in YEARS) {
        for (month: Int in 1..12) {
            val dayOfMonth = i % DAYS_IN_MONTH.getValue(month) + 1
            val hour = i % 24
            val minute = i % 60
            val second = (i * 233).absoluteValue % 60
            val nanoOfSecond = when (allowed) {
                AllowedSubseconds.NANOS -> NANO_FRACTIONS[i % NANO_FRACTIONS.size]
                AllowedSubseconds.MICROS -> MICRO_FRACTIONS[i % MICRO_FRACTIONS.size]
                AllowedSubseconds.MILLIS -> MILLI_FRACTIONS[i % MILLI_FRACTIONS.size]
            }
            result.add(ZonedDateTime.of(year, month, dayOfMonth, hour, minute, second, nanoOfSecond, UTC))
            i += 1
        }
    }

    // Leap years
    result.add(ZonedDateTime.of(2004, 2, 29, 23, 59, 59, 999_000_000, UTC))
    result.add(ZonedDateTime.of(1584, 2, 29, 23, 59, 59, 999_000_000, UTC))

    result.sort()
    return result
}

private fun generateEpochSecondsTests(): List<TestCase> {
    val formatter = DateTimeFormatterBuilder()
        .appendValue(ChronoField.INSTANT_SECONDS, 1, 19, SignStyle.NORMAL)
        .optionalStart()
        .appendFraction(ChronoField.MICRO_OF_SECOND, 0, 6, true)
        .optionalEnd()
        .toFormatter()
    return generateTestTimes(AllowedSubseconds.MICROS).map { time ->
        TestCase(time, formatter.format(time))
    }
}

private fun generateHttpDateTests(parsing: Boolean): List<TestCase> {
    val formatter = DateTimeFormatterBuilder()
        .appendPattern("EEE, dd MMM yyyy HH:mm:ss")
        .optionalStart()
        .appendFraction(ChronoField.MILLI_OF_SECOND, 0, 3, true)
        .optionalEnd()
        .appendLiteral(" GMT")
        .toFormatter(Locale.ENGLISH)
    return generateTestTimes(if (parsing) AllowedSubseconds.MILLIS else AllowedSubseconds.NANOS).map { time ->
        TestCase(
            time,
            when {
                time.year < 0 -> null
                else -> formatter.format(time)
            }
        )
    }
}

private fun generateDateTimeTests(parsing: Boolean): List<TestCase> {
    val formatter = DateTimeFormatterBuilder()
        .appendPattern("yyyy-MM-dd'T'HH:mm:ss")
        .optionalStart()
        .appendFraction(ChronoField.MICRO_OF_SECOND, 0, 6, true)
        .optionalEnd()
        .appendLiteral("Z")
        .toFormatter(Locale.ENGLISH)
    return generateTestTimes(if (parsing) AllowedSubseconds.MICROS else AllowedSubseconds.NANOS).map { time ->
        TestCase(
            time,
            when {
                time.year < 0 -> null
                else -> formatter.format(time)
            }
        )
    }
}

fun main() {
    val none = SourceLocation.NONE
    val topLevels = mapOf<String, Node>(
        "description" to ArrayNode(
            """
            This file holds format and parse test cases for Smithy's built-in `epoch-seconds`,
            `http-date`, and `date-time` timestamp formats.

            There are six top-level sections:
             - `format_epoch_seconds`: Test cases for formatting timestamps into `epoch-seconds`
             - `format_http_date`: Test cases for formatting timestamps into `http-date`
             - `format_date_time`: Test cases for formatting timestamps into `date-time`
             - `parse_epoch_seconds`: Test cases for parsing timestamps from `epoch-seconds`
             - `parse_http_date`: Test cases for parsing timestamps from `http-date`
             - `parse_date_time`: Test cases for parsing timestamps from `date-time`

            Each top-level section is an array of the same test case data structure:
            ```typescript
            type TestCase = {
                // Human-readable ISO-8601 representation of the canonical date-time. This should not
                // be used by tests, and is only present to make test failures more human readable.
                iso8601: string,

                // The canonical number of seconds since the Unix epoch in UTC.
                canonical_seconds: string,

                // The canonical nanosecond adjustment to the canonical number of seconds.
                // If conversion from (canonical_seconds, canonical_nanos) into a 128-bit integer is required,
                // DO NOT just add the two together as this will yield an incorrect value when
                // canonical_seconds is negative.
                canonical_nanos: number,

                // Will be true if this test case is expected to result in an error or exception
                error: boolean,

                // String value of the timestamp in the Smithy format. For the `format_epoch_seconds` top-level,
                // this will be in the `epoch-seconds` format, and for `parse_http_date`, it will be in the
                // `http-date` format (and so on).
                //
                // For parsing tests, parse this value and compare the result against canonical_seconds
                // and canonical_nanos.
                //
                // For formatting tests, form the canonical_seconds and canonical_nanos, and then compare
                // the result against this value.
                //
                // This value will not be set for formatting tests if `error` is set to `true`.
                smithy_format_value: string,
            }
            ```
            """.trimIndent().split("\n").map { Node.from(it) },
            none
        ),
        "format_epoch_seconds" to ArrayNode(generateEpochSecondsTests().map(TestCase::toNode), none),
        "format_http_date" to ArrayNode(generateHttpDateTests(parsing = false).map(TestCase::toNode), none),
        "format_date_time" to ArrayNode(generateDateTimeTests(parsing = false).map(TestCase::toNode), none),
        "parse_epoch_seconds" to ArrayNode(
            generateEpochSecondsTests()
                .filter { it.formatted != null }
                .map(TestCase::toNode),
            none
        ),
        "parse_http_date" to ArrayNode(
            generateHttpDateTests(parsing = true)
                .filter { it.formatted != null }
                .map(TestCase::toNode),
            none
        ),
        "parse_date_time" to ArrayNode(
            generateDateTimeTests(parsing = true)
                .filter { it.formatted != null }
                .map(TestCase::toNode),
            none
        ),
    ).mapKeys { Node.from(it.key) }

    println(Node.prettyPrintJson(ObjectNode(topLevels, none)))
}
