package software.amazon.smithy.rust.codegen.smithy.protocols.parsers

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
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
) : StructuredDataParserGenerator {
    private val symbolProvider = protocolConfig.symbolProvider
    private val smithyXml = CargoDependency.smithyXml(protocolConfig.runtimeConfig).asType()
    private val xmlError = smithyXml.member("decode::XmlError")
    private val xmlBindingGenerator = XmlBindingTraitParserGenerator(protocolConfig, xmlErrors)
    private val codegenScope = arrayOf(
        "Document" to smithyXml.member("decode::Document"),
        "XmlError" to xmlError,
    )

    override fun payloadParser(member: MemberShape): RuntimeType = xmlBindingGenerator.payloadParser(member)
    override fun errorParser(errorShape: StructureShape): RuntimeType? = xmlBindingGenerator.errorParser(errorShape)
    override fun documentParser(operationShape: OperationShape): RuntimeType =
        TODO("Document shapes are not supported by the AWS query protocol")

    override fun operationParser(operationShape: OperationShape): RuntimeType? {
        return xmlBindingGenerator.operationParserWithImpl(operationShape) { members ->
            val operationName = symbolProvider.toSymbol(operationShape).name
            val responseWrapperName = operationName + "Response"
            val resultWrapperName = operationName + "Result"

            rustTemplate(
                """
                use std::convert::TryFrom;
                let mut doc = #{Document}::try_from(inp)?;

                ##[allow(unused_mut)]
                let mut decoder = doc.root_element()?;
                let start_el = decoder.start_el();

                if !(${XmlBindingTraitParserGenerator.XmlName(responseWrapperName).matchExpression("start_el")}) {
                    return Err(#{XmlError}::custom(format!("invalid root, expected $responseWrapperName got {:?}", start_el)))
                }
                if let Some(mut result_tag) = decoder.next_tag() {
                    let start_el = result_tag.start_el();
                    if !(${XmlBindingTraitParserGenerator.XmlName(resultWrapperName).matchExpression("start_el")}) {
                        return Err(#{XmlError}::custom(format!("invalid result, expected $resultWrapperName got {:?}", start_el)))
                    }
                """,
                *codegenScope
            )
            xmlBindingGenerator.parseStructureInner(
                this, members, builder = "builder",
                XmlBindingTraitParserGenerator.Ctx(tag = "result_tag", accum = null)
            )
            rustTemplate(
                """
                } else {
                    return Err(#{XmlError}::custom("expected $resultWrapperName tag"))
                };
                """,
                *codegenScope
            )
            rust("Ok(builder)")
        }
    }
}
