package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust

/**
 * Information needed to render a constraint trait as Rust code.
 */
data class TraitInfo(
    val tryFromCheck: Writable,
    val constraintViolationVariant: Writable,
    val asValidationExceptionField: Writable,
    val validationFunctionDefinition: (constraintViolation: Symbol) -> Writable,
)
