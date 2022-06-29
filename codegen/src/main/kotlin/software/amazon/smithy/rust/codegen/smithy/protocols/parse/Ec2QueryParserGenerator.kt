/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType

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
    coreCodegenContext: CoreCodegenContext,
    xmlErrors: RuntimeType,
    private val xmlBindingTraitParserGenerator: XmlBindingTraitParserGenerator =
        XmlBindingTraitParserGenerator(
            coreCodegenContext,
            xmlErrors
        ) { context, inner ->
            val operationName = coreCodegenContext.symbolProvider.toSymbol(context.shape).name
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
