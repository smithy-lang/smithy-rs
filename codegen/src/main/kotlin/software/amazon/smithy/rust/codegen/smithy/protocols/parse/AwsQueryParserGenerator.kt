/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

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
    coreCodegenContext: CoreCodegenContext,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            coreCodegenContext,
            xmlErrors
        ) { context, inner ->
            val operationName = coreCodegenContext.symbolProvider.toSymbol(context.shape).name
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
