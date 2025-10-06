$version: "2.0"

namespace smithy.rust.codegen.traits

/// Marks a structure as a custom validation exception that can replace
/// smithy.framework#ValidationException in operation error lists.
@trait(selector: "structure")
structure validationException {}

/// Marks a String member as the primary message field for a validation exception.
/// Exactly one member in a @validationException structure must have this trait.
@trait(selector: "structure[trait|smithy.rust.codegen.traits#validationException] > member")
structure validationMessage {}

/// Marks a member as containing the list of field-level validation errors.
/// The target shape must be a String, List<String>, or List<Structure> where
/// the structure contains validation field information.
@trait(selector: "structure > member")
structure validationFieldList {}

/// Marks a String member as containing the field name in a validation field structure.
@trait(selector: "structure > member")
structure validationFieldName {}

/// Marks a String member as containing the field error message in a validation field structure.
@trait(selector: "structure > member")
structure validationFieldMessage {}
