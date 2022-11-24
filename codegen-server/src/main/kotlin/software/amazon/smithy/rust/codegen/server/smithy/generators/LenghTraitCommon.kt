package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.traits.LengthTrait

fun LengthTrait.rustCondition(): String {
    val condition = if (min.isPresent && max.isPresent) {
        "(${min.get()}..=${max.get()}).contains(&length)"
    } else if (min.isPresent) {
        "${min.get()} <= length"
    } else {
        "length <= ${max.get()}"
    }

    return condition
}
