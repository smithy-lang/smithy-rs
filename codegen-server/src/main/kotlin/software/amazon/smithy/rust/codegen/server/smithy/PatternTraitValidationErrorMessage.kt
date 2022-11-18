/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.traits.PatternTrait

@Suppress("UnusedReceiverParameter")
fun PatternTrait.validationErrorMessage(): String {
    return "Value at '{}' failed to satisfy constraint: Member must satisfy regex '{}'."
}

/**
 * Escape `\`s to not end up with broken rust code in the presence of regexes with slashes.
 * This turns `Regex::new("^[\S\s]+$")` into `Regex::new("^[\\S\\s]+$")`.
 */
fun PatternTrait.escapedPattern(): String {
    return this.pattern.toString().replace("\\", "\\\\")
}
