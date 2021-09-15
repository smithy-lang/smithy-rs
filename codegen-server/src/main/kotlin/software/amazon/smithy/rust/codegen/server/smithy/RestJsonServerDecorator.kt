package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolLoader

class RustJsonServerDecorator : RustCodegenDecorator {
    override val name = "RustJsonServerDecorator"
    override val order: Byte = 10

    private fun applies(protocolConfig: ProtocolConfig) =
            RestJson1Trait.ID ==
                    ProtocolLoader.Default.protocolFor(
                                    protocolConfig.model,
                                    protocolConfig.serviceShape
                            )
                            .first

    override fun extras(protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
        if (!applies(protocolConfig)) {
            return
        }

        val module = RustMetadata(public = true)
        rustCrate.withModule(RustModule("json_serde", module)) { writer ->
            RestJsonSerdeGenerator(protocolConfig).render(writer)
        }
    }
}
