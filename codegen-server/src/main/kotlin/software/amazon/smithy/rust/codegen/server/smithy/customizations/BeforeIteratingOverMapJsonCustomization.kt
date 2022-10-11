package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonCustomization
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.JsonSection
import software.amazon.smithy.rust.codegen.server.smithy.workingWithPublicConstrainedWrapperTupleType
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

/**
 * A customization to, just before we iterate over a _constrained_ map shape in a JSON serializer, unwrap the wrapper
 * newtype and take a shared reference to the actual `std::collections::HashMap` within it.
 */
class BeforeIteratingOverMapJsonCustomization(private val codegenContext: ServerCodegenContext) : JsonCustomization() {
    override fun section(section: JsonSection): Writable = when (section) {
        is JsonSection.ServerError -> emptySection
        is JsonSection.BeforeIteratingOverMap -> writable {
            if (workingWithPublicConstrainedWrapperTupleType(
                    section.shape,
                    codegenContext.model,
                    codegenContext.settings.codegenConfig.publicConstrainedTypes,
                )
            ) {
                // Note that this particular implementation just so happens to work because when the customization
                // is invoked in the JSON serializer, the value expression is guaranteed to be a variable binding name.
                // If the expression in the future were to be more complex, we wouldn't be able to write the left-hand
                // side of this assignment.
                rust("""let ${section.valueExpression.name} = &${section.valueExpression.name}.0;""")
            }
        }
    }
}
