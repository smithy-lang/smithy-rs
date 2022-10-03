package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.client.rustlang.RustType
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.rustType

// TODO(https://github.com/awslabs/smithy-rs/issues/1724): This file is a placeholder for when the server project adds
//  its own bottom-most symbol provider.

data class MaybeConstrained(override val member: RustType, val runtimeConfig: RuntimeConfig) : RustType(), RustType.Container {
    private val runtimeType: RuntimeType = ServerRuntimeType.MaybeConstrained(runtimeConfig)
    override val name = runtimeType.name!!
    override val namespace = runtimeType.namespace
}

/**
 * Make the Rust type of a symbol wrapped in `MaybeConstrained`. (hold `MaybeConstrained<T>`).
 *
 * This is idempotent and will have no change if the type is already `MaybeConstrained<T>`.
 */
fun Symbol.makeMaybeConstrained(runtimeConfig: RuntimeConfig): Symbol =
    if (this.rustType() is MaybeConstrained) {
        this
    } else {
        val rustType = MaybeConstrained(this.rustType(), runtimeConfig)
        Symbol.builder()
            .rustType(rustType)
            .addReference(this)
            .name(rustType.name)
            .build()
    }

