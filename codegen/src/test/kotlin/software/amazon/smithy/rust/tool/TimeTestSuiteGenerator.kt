/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.tool

import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.DateTimeFormatterBuilder
import java.time.format.SignStyle
import java.time.temporal.ChronoField
import java.util.Locale
import java.util.TreeMap
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
    fun toSerializable(): SerializableTestCase =
        time.toInstant().let { instant ->
            SerializableTestCase(
                DateTimeFormatter.ISO_OFFSET_DATE_TIME.format(time),
                formatted,
                instant.epochSecond,
                instant.nano,
            )
        }
}

private data class SerializableTestCase(
    val iso: String,
    val formatted: String?,
    val epochSeconds: Long,
    val epochSubsecondNanos: Int,
)

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
    val tests = TreeMap<String, List<SerializableTestCase>>()
    tests["format_epoch_seconds"] = generateEpochSecondsTests().map(TestCase::toSerializable)
    tests["format_http_date"] = generateHttpDateTests(parsing = false).map(TestCase::toSerializable)
    tests["format_date_time"] = generateDateTimeTests(parsing = false).map(TestCase::toSerializable)
    tests["parse_epoch_seconds"] = generateEpochSecondsTests()
        .map(TestCase::toSerializable)
        .filter { it.formatted != null }
    tests["parse_http_date"] = generateHttpDateTests(parsing = true)
        .map(TestCase::toSerializable)
        .filter { it.formatted != null }
    tests["parse_date_time"] = generateDateTimeTests(parsing = true)
        .map(TestCase::toSerializable)
        .filter { it.formatted != null }

    println("{")
    val testSuites = tests.entries.map { entry ->
        var result = "  \"${entry.key}\": [\n"
        val testCases = entry.value.map { testCase ->
            val iso = testCase.iso.replace("\"", "\\\"")
            val formatted = testCase.formatted?.let { "\"" + it.replace("\"", "\\\"") + "\"" } ?: "null"
            val secs = testCase.epochSeconds
            val subsecs = testCase.epochSubsecondNanos
            """    { "epoch_seconds": $secs, "epoch_subsecond_nanos": $subsecs, "iso": "$iso",
              |      "formatted": $formatted }""".trimMargin()
        }
        result += testCases.joinToString(",\n")
        result += "\n  ]"
        result
    }
    println(testSuites.joinToString(",\n"))
    println("}")
}
