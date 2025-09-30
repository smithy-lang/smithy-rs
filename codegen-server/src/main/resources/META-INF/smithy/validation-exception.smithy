$version: "2.0"

namespace smithy.rust.codegen.server.traits

/// Marks a structure as a custom validation exception that can be used
/// instead of smithy.framework#ValidationException in operation error lists.
@trait(selector: "structure")
structure validationException {}

/// Marks a String member as the primary message field for a validation exception.
/// This trait can be applied in two ways:
/// 1. Explicitly: By adding this trait to any String member
/// 2. Implicitly: If a field named "message" exists and no other field has this trait
///
/// Note: A structure marked with @validationException MUST have exactly one field
/// that is either explicitly marked with this trait or implicitly selected via the
/// "message" field name convention.
@trait(selector: "structure[trait|smithy.rust.codegen.server.traits#validationException] > member")
structure validationMessage {}

/// Marks a member as containing the list of field-level validation errors.
/// The target shape must be a String, List<String>, or List<Structure> where
/// the structure contains validation field information.
@trait(selector: "structure > member")
structure validationFieldList {}

/// Marks a String member that will be automatically populated by the framework
/// with the Shape ID of the field that failed validation. The framework will
/// use the complete Shape ID (e.g., "MyService#MyStructure$fieldName").
@trait(selector: "structure > member")
structure validationFieldName {}

/// Marks a String member as containing the field error message in a validation field structure.
@trait(selector: "structure > member")
structure validationFieldMessage {}
