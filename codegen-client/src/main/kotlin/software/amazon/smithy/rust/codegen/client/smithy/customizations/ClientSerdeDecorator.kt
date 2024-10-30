package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.serde.extrasCommon

class ClientSerdeDecorator : ClientCodegenDecorator {
    override val name: String = "ClientSerdeDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) = extrasCommon(
        codegenContext,
        rustCrate,
        constraintTraitsEnabled = false,
        unwrapConstraints = { writable { } },
        hasConstraintTrait = { _ -> false },
    )
}

