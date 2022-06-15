package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget

/**
 * TODO Docs
 *
 * This class has to live in the `codegen` subproject because it is referenced in common generators to both client
 * and server (like [JsonParserGenerator]).
 */
data class ServerCodegenContext(
    override val model: Model,
    override val symbolProvider: RustSymbolProvider,
    override val serviceShape: ServiceShape,
    override val protocol: ShapeId,
    override val settings: ServerRustSettings,
    override val target: CodegenTarget,
    val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider
) : CodegenContext(
    model, symbolProvider, serviceShape, protocol, settings, target
)