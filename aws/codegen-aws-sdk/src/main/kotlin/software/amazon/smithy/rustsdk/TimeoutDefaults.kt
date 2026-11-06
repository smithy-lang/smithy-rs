/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.ObjectNode
import java.time.Duration

/**
 * Per-service default read timeout, resolved from the shared timeout-defaults artifact.
 *
 * The artifact (`aws/sdk/aws-models/sdk-timeout-defaults.json`) is the cross-SDK source of truth and
 * is also checked into aws-sdk-go-v2 and aws-sdk-java-v2. It carries a general default plus an
 * override per service, where `-1` means "apply no read timeout to this service at all".
 *
 * Rust resolves it at the same per-service granularity the other SDKs use, and overrides exactly
 * four entries — see [RUST_GENERAL_DEFAULT_OVERRIDES].
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
         * `read_timeout` closes when response headers arrive, and every operation in these four
         * services sends headers promptly — Transcribe Streaming measured at 778ms — so the
         * exemption would remove blackhole protection without preventing any false positive.
         *
         * Design section 2.3 has the derivation and the per-operation audit behind it. Changing
         * this set is a behavior change; `TimeoutDefaultsTest` checks each name still resolves to
         * an exempt artifact entry, because a drifted name fails silently.
         */
        val RUST_GENERAL_DEFAULT_OVERRIDES =
            setOf(
                "Transcribe Streaming",
                "ConnectHealth",
                "Polly",
                "SageMaker Runtime HTTP2",
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
     * The default read timeout for [sdkId], or `null` if no read timeout should be applied.
     *
     * [sdkId] is the `aws.api#service` `sdkId` trait value, matched exactly — the artifact is keyed
     * by the same string the other SDKs key their maps by.
     */
    fun readTimeoutFor(sdkId: String): Duration? = if (perService.containsKey(sdkId)) perService[sdkId] else generalDefault

    private fun millis(value: Long): Duration {
        require(value > 0) {
            "timeout value must be positive or the $EXEMPT exemption sentinel, got $value"
        }
        return Duration.ofMillis(value)
    }
}
