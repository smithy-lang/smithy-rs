package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

class RestXmlParserGenerator(
    codegenContext: CodegenContext,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            codegenContext,
            xmlErrors
        ) { context, inner ->
            val shapeName = context.outputShapeName
            rustTemplate(
                """
                if !(${XmlBindingTraitParserGenerator.XmlName(shapeName).matchExpression("start_el")}) {
                    return Err(#{XmlError}::custom(format!("invalid root, expected $shapeName got {:?}", start_el)))
                }
                """,
                "XmlError" to context.xmlErrorType
            )
            inner("decoder")
        }
) : StructuredDataParserGenerator by xmlBindingTraitParserGenerator
