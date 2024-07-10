/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.codegen.core.Symbol

/**
 * Given a shape, parsers need to know the symbol to parse and return, and whether it's unconstrained or not.
 */
data class ReturnSymbolToParse(val symbol: Symbol, val isUnconstrained: Boolean)
