/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.protocol

data class ProtocolSupport(
    /* Client support */
    val requestSerialization: Boolean,
    val requestBodySerialization: Boolean,
    val responseDeserialization: Boolean,
    val errorDeserialization: Boolean,
    /* Server support */
    val requestDeserialization: Boolean,
    val requestBodyDeserialization: Boolean,
    val responseSerialization: Boolean,
    val errorSerialization: Boolean,
)
