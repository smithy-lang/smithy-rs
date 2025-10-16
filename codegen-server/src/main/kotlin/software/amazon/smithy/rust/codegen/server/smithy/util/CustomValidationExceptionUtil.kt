/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.util

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.traits.ValidationFieldNameTrait
import software.amazon.smithy.rust.codegen.traits.ValidationMessageTrait

/**
 * Helper function to determine if this [MemberShape] is a validation message either explicitly with the
 * @validationMessage trait or implicitly because it is named "message"
 */
fun MemberShape.isValidationMessage(): Boolean {
    return this.hasTrait(ValidationMessageTrait.ID) || this.memberName == "message"
}

fun MemberShape.isValidationFieldName(): Boolean {
    return this.hasTrait(ValidationFieldNameTrait.ID) || this.memberName == "name"
}
