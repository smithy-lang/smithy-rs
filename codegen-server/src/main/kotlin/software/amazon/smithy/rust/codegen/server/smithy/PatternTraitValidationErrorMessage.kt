/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.model.traits.PatternTrait

@Suppress("UnusedReceiverParameter")
fun PatternTrait.validationErrorMessage(): String =
    "Value {} at '{}' failed to satisfy constraint: Member must satisfy regular expression pattern: {}"
