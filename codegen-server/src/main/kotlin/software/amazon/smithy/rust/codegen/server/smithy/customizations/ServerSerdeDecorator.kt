package software.amazon.smithy.rust.codegen.server.smithy.customizations

import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.serde.extrasCommon
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.hasConstraintTrait

class ServerSerdeDecorator : ServerCodegenDecorator {
    override val name: String = "ServerSerdeDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        val constraintTraitsEnabled = codegenContext.settings.codegenConfig.publicConstrainedTypes

        extrasCommon(
            codegenContext,
            rustCrate,
            constraintTraitsEnabled = constraintTraitsEnabled,
            unwrapConstraints = { shape ->
                writable {
                    if (constraintTraitsEnabled && shape.hasConstraintTrait()) {
                        if (shape.isBlobShape || shape.isTimestampShape || shape.isDocumentShape || shape is NumberShape) {
                            rust(".0")
                        }
                    }
                }
            },
            hasConstraintTrait = { shape -> shape.hasConstraintTrait() }
        )
    }
}


