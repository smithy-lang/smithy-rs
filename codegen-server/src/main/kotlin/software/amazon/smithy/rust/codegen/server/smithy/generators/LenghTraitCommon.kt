/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.traits.LengthTrait

fun LengthTrait.rustCondition(lengthVariable: String): String {
    val condition = if (min.isPresent && max.isPresent) {
        "(${min.get()}..=${max.get()}).contains(&$lengthVariable)"
    } else if (min.isPresent) {
        "${min.get()} <= $lengthVariable"
    } else {
        "$lengthVariable <= ${max.get()}"
    }

    return condition
}
