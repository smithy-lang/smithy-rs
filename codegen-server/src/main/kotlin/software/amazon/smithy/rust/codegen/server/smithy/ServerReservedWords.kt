/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWordConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator

val ServerReservedWords = RustReservedWordConfig(
    structureMemberMap = StructureGenerator.structureMemberNameMap,
    unionMemberMap = emptyMap(),
    enumMemberMap = emptyMap(),
)
