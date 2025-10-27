/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

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
    codegenContext: CodegenContext,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            codegenContext,
            xmlErrors,
        ) { context, inner ->
            val operationName = codegenContext.symbolProvider.toSymbol(context.shape).name
            val responseWrapperName = operationName + "Response"
            val resultWrapperName = operationName + "Result"
            rustTemplate(
                """
                if !(${XmlBindingTraitParserGenerator.XmlName(responseWrapperName).matchExpression("start_el")}) {
                    return Err(#{XmlDecodeError}::custom(format!("invalid root, expected $responseWrapperName got {start_el:?}")))
                }
                if let Some(mut result_tag) = decoder.next_tag() {
                    let start_el = result_tag.start_el();
                    if !(${XmlBindingTraitParserGenerator.XmlName(resultWrapperName).matchExpression("start_el")}) {
                        return Err(#{XmlDecodeError}::custom(format!("invalid result, expected $resultWrapperName got {start_el:?}")))
                    }
                """,
                "XmlDecodeError" to context.xmlDecodeErrorType,
            )
            inner("result_tag")
            rustTemplate(
                """
                } else {
                    return Err(#{XmlDecodeError}::custom("expected $resultWrapperName tag"))
                };
                """,
                "XmlDecodeError" to context.xmlDecodeErrorType,
            )
        },
) : StructuredDataParserGenerator by xmlBindingTraitParserGenerator
