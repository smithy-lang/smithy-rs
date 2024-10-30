package software.amazon.smithy.rust.codegen.core.smithy.customizations.serde

import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.smithy.shapes.SerdeTrait

val SerdeFeature = Feature("serde", false, listOf("dep:serde"))
val SerdeModule =
    RustModule.public(
        "serde",
        additionalAttributes = listOf(Attribute.featureGate(SerdeFeature.name)),
        documentationOverride = "Implementations of `serde` for model types. NOTE: These implementations are NOT used for wire serialization as part of a Smithy protocol and WILL NOT match the wire format. They are provided for convenience only.",
    )

/**
 * The entrypoint to both the client and server decorators.
 */
fun extrasCommon(
    codegenContext: CodegenContext,
    rustCrate: RustCrate,
    constraintTraitsEnabled: Boolean,
    unwrapConstraints: (Shape) -> Writable,
    hasConstraintTrait: (Shape) -> Boolean,
) {
    val roots = serializationRoots(codegenContext)
    if (roots.isNotEmpty()) {
        rustCrate.mergeFeature(SerdeFeature)
        val generator =
            SerializeImplGenerator(
                codegenContext,
                constraintTraitsEnabled,
                unwrapConstraints,
                hasConstraintTrait,
            )
        rustCrate.withModule(SerdeModule) {
            roots.forEach {
                generator.generateRootSerializerForShape(it)(this)
            }
            addDependency(SupportStructures.serializeRedacted().toSymbol())
            addDependency(SupportStructures.serializeUnredacted().toSymbol())
        }
    }
}

/**
 * All entry points for serialization in the service closure.
 */
fun serializationRoots(ctx: CodegenContext): List<Shape> {
    val serviceShape = ctx.serviceShape
    val walker = DirectedWalker(ctx.model)
    return walker.walkShapes(serviceShape).filter { it.hasTrait<SerdeTrait>() }
}
