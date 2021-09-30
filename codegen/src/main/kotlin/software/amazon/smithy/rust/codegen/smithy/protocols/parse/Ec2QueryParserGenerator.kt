package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolConfig

/**
 * The EC2 query protocol's responses are identical to REST XML's, except that they are wrapped
 * in a Response tag:
 *
 * ```
 * <SomeOperationResponse>
 *    <ActualData /> <!-- This part is the same as REST XML -->
 * </SomeOperationResponse>
 * ```
 *
 * This class wraps [XmlBindingTraitParserGenerator] and uses it to render the vast majority
 * of the response parsing, but it overrides [operationParser] to add the protocol differences.
 */
class Ec2QueryParserGenerator(
    protocolConfig: ProtocolConfig,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            protocolConfig,
            xmlErrors
        ) { context, inner ->
            val operationName = protocolConfig.symbolProvider.toSymbol(context.shape).name
            val responseWrapperName = operationName + "Response"
            rustTemplate(
                """
                if !(${XmlBindingTraitParserGenerator.XmlName(responseWrapperName).matchExpression("start_el")}) {
                    return Err(#{XmlError}::custom(format!("invalid root, expected $responseWrapperName got {:?}", start_el)))
                }
                """,
                "XmlError" to context.xmlErrorType
            )
            inner("decoder")
        }
) : StructuredDataParserGenerator by xmlBindingTraitParserGenerator
