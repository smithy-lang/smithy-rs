package software.amazon.smithy.rust.codegen.server.smithy.protocols

import java.util.logging.Logger
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.Instantiator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver

abstract class ServerGenerator(
                protocolConfig: ProtocolConfig,
                private val httpBindingResolver: HttpTraitHttpBindingResolver,
) {
        public val logger = Logger.getLogger(javaClass.name)
        public val error = RuntimeType("error", null, "crate")
        public val operation = RuntimeType("operation", null, "crate")
        public val runtimeConfig = protocolConfig.runtimeConfig
        public val model = protocolConfig.model
        public val symbolProvider = protocolConfig.symbolProvider
        public val instantiator =
                        with(protocolConfig) { Instantiator(symbolProvider, model, runtimeConfig) }
        public val smithyHttp = CargoDependency.SmithyHttp(runtimeConfig).asType()
        public val index = HttpBindingIndex.of(model)
        public val service = protocolConfig.serviceShape
        public val defaultTimestampFormat = TimestampFormatTrait.Format.EPOCH_SECONDS

        abstract fun render(writer: RustWriter, operationShape: OperationShape)
}
