package software.amazon.smithy.rust.codegen.server.smithy.util

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.server.smithy.traits.ValidationMessageTrait

/**
 * Helper function to determine if this [MemberShape] is a validation message either explicitly with the
 * @validationMessage trait or implicitly because it is named "message"
 */
fun MemberShape.isValidationMessage(): Boolean {
    return this.hasTrait(ValidationMessageTrait.ID) || this.memberName == "message"
}
