package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

/**
 * The AWS query protocol's responses are identical to REST XML's, except that they are wrapped
 * in a Response/Result tag pair:
 *
 * ```
 * <SomeOperationResponse>
 *     <SomeOperationResult>
 *         <ActualData /> <!-- This part is the same as REST XML -->
 *     </SomeOperationResult>
 * </SomeOperationResponse>
 * ```
 *
 * This class wraps [XmlBindingTraitParserGenerator] and uses it to render the vast majority
 * of the response parsing, but it overrides [operationParser] to add the protocol differences.
 */
class AwsQueryParserGenerator(
    protocolConfig: ProtocolConfig,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            protocolConfig,
            xmlErrors
        ) { context, inner ->
            val operationName = protocolConfig.symbolProvider.toSymbol(context.shape).name
            val responseWrapperName = operationName + "Response"
            val resultWrapperName = operationName + "Result"
            rustTemplate(
                """
                if !(${XmlBindingTraitParserGenerator.XmlName(responseWrapperName).matchExpression("start_el")}) {
                    return Err(#{XmlError}::custom(format!("invalid root, expected $responseWrapperName got {:?}", start_el)))
                }
                if let Some(mut result_tag) = decoder.next_tag() {
                    let start_el = result_tag.start_el();
                    if !(${XmlBindingTraitParserGenerator.XmlName(resultWrapperName).matchExpression("start_el")}) {
                        return Err(#{XmlError}::custom(format!("invalid result, expected $resultWrapperName got {:?}", start_el)))
                    }
                """,
                "XmlError" to context.xmlErrorType
            )
            inner("result_tag")
            rustTemplate(
                """
                } else {
                    return Err(#{XmlError}::custom("expected $resultWrapperName tag"))
                };
                """,
                "XmlError" to context.xmlErrorType
            )
        }
) : StructuredDataParserGenerator by xmlBindingTraitParserGenerator
