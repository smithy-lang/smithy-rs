/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.AddedDefaultTrait
import software.amazon.smithy.model.traits.ClientOptionalTrait
import software.amazon.smithy.model.traits.InputTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.util.hasTrait

class SerializerUtil(private val model: Model, private val symbolProvider: SymbolProvider) {
    fun RustWriter.ignoreDefaultsForNumbersAndBools(
        shape: MemberShape,
        value: ValueExpression,
        inner: Writable,
    ) {
        // @required shapes should always be serialized, and members with @clientOptional or part of @input structures
        // should ignore default values. If we have an Option<T>, it won't have a default anyway, so we don't need to
        // ignore it.
        // See https://github.com/smithy-lang/smithy-rs/issues/230 and https://github.com/aws/aws-sdk-go-v2/pull/1129
        val container = model.expectShape(shape.container)
        if (
            shape.isRequired ||
            shape.hasTrait<ClientOptionalTrait>() ||
            // Treating `addedDefault` as `default` could break existing serializers.
            // Fields that were previously marked as `required` but later replaced with `addedDefault`
            // may no longer be serialized. This could occur when an explicitly set value matches the default.
            shape.hasTrait<AddedDefaultTrait>() ||
            // Zero values are always serialized in lists and collections, this only applies to structures
            container !is StructureShape ||
            container.hasTrait<InputTrait>()
        ) {
            rustBlock("") {
                inner(this)
            }
        } else {
            this.ifNotNumberOrBoolDefault(model.expectShape(shape.target), symbolProvider.toSymbol(shape), value) {
                inner(this)
            }
        }
    }
}
