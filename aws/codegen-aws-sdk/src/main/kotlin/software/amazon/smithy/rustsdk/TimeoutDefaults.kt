/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.util.PANIC
import java.time.Duration

/**
 * Per-service default read timeout, resolved from the shared timeout-defaults artifact.
 *
 * The artifact (`aws/sdk/sdk-timeout-defaults.json`) is the cross-SDK source of truth and
 * is also checked into aws-sdk-go-v2 and aws-sdk-java-v2. It carries a general default plus an
 * override per service, where `-1` means "apply no read timeout to this service at all".
 *
 * Rust resolves it at the same per-service granularity the other SDKs use, and overrides exactly
 * three entries — see [RUST_GENERAL_DEFAULT_OVERRIDES].
 */
class TimeoutDefaults(artifact: ObjectNode) {
    companion object {
        /** Sentinel in the artifact meaning "no read timeout for this service". */
        private const val EXEMPT = -1L

        /**
         * Services the artifact fully exempts but which Rust gives the general default instead.
         *
         * The artifact exempts these because Go's and Java's timeouts span the whole response
         * lifecycle, so they would fire partway through a healthy bidirectional stream. Rust's
         * closes when response headers arrive, and all three send headers in tens of milliseconds
         * — median 69ms, 13ms and 76ms respectively — so exempting them would give up blackhole
         * protection without preventing any false positive.
         *
         * Changing this set is a behavior change. `TimeoutDefaultsTest` checks each name still
         * resolves to an exempt artifact entry, because a drifted name fails silently.
         */
        val RUST_GENERAL_DEFAULT_OVERRIDES =
            setOf(
                "Transcribe Streaming",
                "ConnectHealth",
                "Polly",
            )
    }

    private val generalDefault: Duration = millis(artifact.expectNumberMember("generalDefaultMillis").value.toLong())

    /** sdkId to resolved timeout; absent value means fully exempt. */
    private val perService: Map<String, Duration?> =
        artifact.expectObjectMember("services").members.entries.associate { (key, value) ->
            val sdkId = key.value
            val raw = value.expectNumberNode().value.toLong()
            sdkId to
                when {
                    sdkId in RUST_GENERAL_DEFAULT_OVERRIDES -> generalDefault
                    raw == EXEMPT -> null
                    else -> millis(raw)
                }
        }

    /**
     * Normalized key to original key, for the drift check in [readTimeoutFor].
     *
     * Built eagerly so two keys normalizing alike fail when the artifact loads, rather than at
     * whichever service happens to resolve first.
     */
    private val normalizedKeys: Map<String, String> =
        perService.keys.groupBy { normalize(it) }.mapValues { (_, keys) ->
            if (keys.size > 1) {
                PANIC(
                    "the timeout-defaults artifact has ${keys.size} keys that differ only by case " +
                        "or punctuation: ${keys.sorted()}. At most one can ever match a service " +
                        "sdkId, so the rest are dead entries. Fix this in the shared artifact.",
                )
            }
            keys.single()
        }

    /**
     * The default read timeout for [sdkId], or `null` if no read timeout should be applied.
     *
     * [sdkId] is the `aws.api#service` `sdkId` trait value, matched exactly — the artifact is keyed
     * by the same string the other SDKs key their maps by.
     *
     * A service with no entry takes the general default. That is the common case and not an error;
     * only 83 of several hundred services are listed. An *almost* matching key is an error.
     */
    fun readTimeoutFor(sdkId: String): Duration? {
        if (perService.containsKey(sdkId)) {
            return perService[sdkId]
        }
        // No exact entry. Check that is genuine absence rather than a key that drifted out of
        // alignment with this sdkId, which would silently downgrade the service to the general
        // default or strip its exemption.
        normalizedKeys[normalize(sdkId)]?.let { drifted ->
            PANIC(
                "the timeout-defaults artifact has the key '$drifted', which differs from the " +
                    "actual service sdkId '$sdkId' only by case or punctuation. The entry can " +
                    "never apply, so '$sdkId' would silently take the general default. Correct " +
                    "the key in aws/sdk/sdk-timeout-defaults.json to match exactly.",
            )
        }
        return generalDefault
    }

    /** Case- and punctuation-insensitive form of a key, for drift detection only. */
    private fun normalize(key: String) = key.lowercase().filter { it.isLetterOrDigit() }

    private fun millis(value: Long): Duration {
        require(value > 0) {
            "timeout value must be positive or the $EXEMPT exemption sentinel, got $value"
        }
        return Duration.ofMillis(value)
    }
}
