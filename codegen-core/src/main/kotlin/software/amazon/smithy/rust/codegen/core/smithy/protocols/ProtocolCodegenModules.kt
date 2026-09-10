/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols

import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.eventStreamSerdeModule

/** Modules that own generated protocol serde and event-stream support code. */
data class ProtocolCodegenModules(
    val serde: RustModule.LeafModule,
    val eventStreamSerde: RustModule.LeafModule,
) {
    companion object {
        /** Legacy top-level module layout used by default. */
        val Default: ProtocolCodegenModules =
            ProtocolCodegenModules(
                serde = RustModule.pubCrate("protocol_serde"),
                eventStreamSerde = RustModule.eventStreamSerdeModule(),
            )

        /** Creates the standard protocol serde and event-stream serde modules under [protocolRoot]. */
        fun under(protocolRoot: RustModule.LeafModule): ProtocolCodegenModules =
            ProtocolCodegenModules(
                serde = RustModule.private("protocol_serde", parent = protocolRoot),
                eventStreamSerde = RustModule.private("event_stream_serde", parent = protocolRoot),
            )
    }
}
