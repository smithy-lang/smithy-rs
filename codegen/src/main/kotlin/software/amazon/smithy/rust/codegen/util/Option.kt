package software.amazon.smithy.rust.codegen.util

import java.util.Optional

fun <T> Optional<T>.orNull(): T? = this.orElse(null)
