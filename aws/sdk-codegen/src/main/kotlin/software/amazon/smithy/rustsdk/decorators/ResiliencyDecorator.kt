package software.amazon.smithy.rustsdk.decorators

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rustsdk.AwsCustomization
import software.amazon.smithy.rustsdk.AwsSection
import software.amazon.smithy.rustsdk.awsHttp

class ResiliencyDecorator : AwsCodegenDecorator {
    override val name: String = "Resiliency"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + RetryClassifierFeature(codegenContext.runtimeConfig)
    }

    override fun awsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<AwsCustomization>,
    ): List<AwsCustomization> {
        return baseCustomizations +
            RetryConfigFromSdkConfig() +
            TimeoutConfigFromSdkConfig()
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class RetryClassifierFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun retryType(): RuntimeType = runtimeConfig.awsHttp().member("retry::AwsResponseRetryClassifier")
    override fun section(section: OperationSection) = when (section) {
        is OperationSection.FinalizeOperation -> writable {
            rust(
                "let ${section.operation} = ${section.operation}.with_retry_classifier(#T::new());",
                retryType(),
            )
        }
        else -> emptySection
    }
}

class RetryConfigFromSdkConfig : AwsCustomization() {
    override fun section(section: AwsSection): Writable = writable {
        when (section) {
            is AwsSection.FromSdkConfigForBuilder -> rust("builder.set_retry_config(input.retry_config().cloned());")
        }
    }
}

class TimeoutConfigFromSdkConfig : AwsCustomization() {
    override fun section(section: AwsSection): Writable = writable {
        when (section) {
            is AwsSection.FromSdkConfigForBuilder -> rust("builder.set_timeout_config(input.timeout_config().cloned());")
        }
    }
}
