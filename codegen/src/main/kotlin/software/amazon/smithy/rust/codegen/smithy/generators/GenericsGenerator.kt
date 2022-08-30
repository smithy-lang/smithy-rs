package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.Optional

class GenericsGenerator(
    private val types: MutableList<Pair<String, Optional<RuntimeType>>>
) {
    fun add(type: Pair<String, RuntimeType>) {
        types.add(type.first to Optional.of(type.second))
    }
    
    fun declaration() = writable {
        // Write nothing if this generator is empty
        if (types.isNotEmpty()) {
            val typeArgs = types.joinToString(", ") { it.first }
            rust("<$typeArgs>")
        }
    }
    
    fun bounds() = writable {
        // Only write bounds for generic type params with a bound
        types.filter { it.second.isPresent }.map {
            val (typeArg, runtimeType) = it
            rustTemplate("$typeArg: #{runtimeType},\n", "runtimeType" to runtimeType.orNull()!!)
        }
    }

    operator fun plus(operationGenerics: GenericsGenerator): GenericsGenerator {
        return GenericsGenerator(listOf(types, operationGenerics.types).flatten().toMutableList())
    }
}