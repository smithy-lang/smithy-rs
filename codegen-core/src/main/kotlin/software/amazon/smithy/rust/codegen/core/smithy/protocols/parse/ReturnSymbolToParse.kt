/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.codegen.core.Symbol

/**
 * Parsers need to know what symbol to parse and return, and whether it's unconstrained or not.
 * This data class holds this information that the parsers fill out from a shape.
 */
data class ReturnSymbolToParse(val symbol: Symbol, val isUnconstrained: Boolean)
