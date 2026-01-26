RFC: Custom Validation Exception
================================

> Status: RFC
>
> Applies to: server

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC defines a mechanism to use custom validation exception shapes instead of the standard `smithy.framework#ValidationException`, enabling service teams to maintain backward compatibility with existing APIs or use validation exceptions that have already been published to external consumers. This addresses scenarios where service teams are migrating from non-Smithy models or need to provide validation error responses in a format that differs from the standard Smithy validation exception structure.

Terminology
-----------

- **Constrained shape**: a shape that is either:
  1. a shape with a [constraint trait] attached
  2. a (member) shape with a [`required` trait] attached  
  3. an [`enum` shape]
  4. an [`intEnum` shape]
  5. a [`structure` shape] with at least one required member shape; or
  6. a shape whose closure includes any of the above.
- **ValidationException**: A Smithy error shape that is serialized in the response when constraint validation fails during request processing.
- **Shape closure**: the set of shapes a shape can "reach", including itself.
- **Custom validation exception**: A user-defined error shape marked with validation-specific traits that replaces the standard `smithy.framework#ValidationException`.

[constraint trait]: https://smithy.io/2.0/spec/constraint-traits.html
[`required` trait]: https://smithy.io/2.0/spec/type-refinement-traits.html#required-trait
[`enum` shape]: https://smithy.io/2.0/spec/simple-types.html#enum
[`intEnum` shape]: https://smithy.io/2.0/spec/simple-types.html#intenum
[`structure` shape]: https://smithy.io/2.0/spec/aggregate-types.html#structure

The user experience if this RFC is implemented
----------------------------------------------

Currently, if there is a constrained shape in the input shape closure of an operation, the operation **must** add `smithy.framework#ValidationException` to the operation's error list, otherwise a build error is raised:

```
Caused by: ValidationResult(shouldAbort=true, messages=[LogMessage(level=SEVERE, message=Operation com.aws.example#GetStorage takes in input that is constrained (https://smithy.io/2.0/spec/constraint-traits.html), and as such can fail with a validation exception. You must model this behavior in the operation shape in your model file.
```smithy
use smithy.framework#ValidationException

operation GetStorage {
    ...
    errors: [..., ValidationException] // <-- Add this.
}
```)])
```

The current solution requires adding the standard `ValidationException` to the errors list:

```smithy
operation SomeOperation {
    // <...input / output definition...>
    errors: [
        // <...other errors...>
        ValidationException
    ]
}
```

### Problems with the current approach

Service teams face several challenges with the mandatory use of `smithy.framework#ValidationException`:

1. **Backward compatibility**: Teams migrating existing APIs to Smithy cannot maintain their existing validation error format
2. **Published APIs**: Teams that have already published validation exception schemas to external consumers cannot change the response format without breaking clients
3. **Custom error handling**: Teams may need additional fields or different field names for their validation errors

### Solution: Custom validation exception traits

Once this RFC is implemented, service developers will be able to define custom validation exceptions using the following approach:

#### 1. Define a custom validation exception shape

Apply the `@validationException` trait to any structure shape that is also marked with the `@error` trait:

```smithy
@validationException
@error("client")
structure CustomValidationException {
    // Structure members defined below
}
```

#### 2. Specify the message field (required)

The custom validation exception **must** have **exactly one** String member marked with the `@validationMessage` trait to serve as the primary error message:

```smithy
@validationException
@error("client")
structure CustomValidationException {
    @validationMessage
    @required
    message: String

    // <... other fields ...>
}
```

#### 3. Default constructibility requirement

For the initial implementation, the custom validation exception structure **must** be default constructible. This means the shape either:

  1. **must not** contain any constrained shapes that the framework cannot construct; or
  2. any constrained shapes **must** have default values specified

```smithy
@validationException
@error("client")
structure CustomValidationException {
    @validationMessage
    @required
    message: String,

    @default("VALIDATION_ERROR")
    errorCode: String,

    @default("ErrorInValidation")
    errorKind: ErrorKind
}

enum ErrorKind {
    SomeOtherError,
    ErrorInValidation
}
```

#### 4. Optional field list support

Optionally, the custom validation exception **may** include a field marked with `@validationFieldList` to provide detailed information about which fields failed validation. This **must** be a list shape where the member is a structure shape with detailed field information:

* **must** have a String member marked with `@validationFieldName`
* **may** have a String member marked with `@validationFieldMessage`
* Regarding additional fields:
  * The structure may have no additional fields beyond those specified above, or
  * If additional fields are present, each must be default constructible

```smithy
@validationException
@error("client")
structure CustomValidationException {
    @validationMessage
    @required
    message: String,

    @default("VALIDATION_ERROR")
    errorCode: String,

    @validationFieldList
    fieldErrors: ValidationFieldList
}

list ValidationFieldList {
    member: ValidationField
}

structure ValidationField {
    @validationFieldName
    @required
    fieldName: String,

    @validationFieldMessage
    @required
    errorMessage: String
}
```

#### 5. Using custom validation exceptions in operations

Replace `smithy.framework#ValidationException` with the custom validation exception in operation error lists:

```smithy
operation GetUser {
    input: GetUserInput,
    output: GetUserOutput,
    errors: [
        CustomValidationException,  // Instead of ValidationException
        UserNotFoundError
    ]
}
```

#### 6. Using different validation exceptions in operations

For the initial implementation, we're adopting a simplified approach: validation exceptions cannot be mixed within a service. This means:

* All operations within a service must use the same custom validation exception type, or
* All operations must use the standard Smithy validation exception

While implementing support for multiple validation exception types would not be technically difficult, we've chosen to defer this complexity for the time being.

Future enhancement: If developers need to use shapes from imported models that use either custom or standard Smithy validation exceptions, we plan to add a customization flag that will allow mapping these imported exceptions to a service's preferred exception type. This will enable greater flexibility when working with mixed models while maintaining consistency within a given service.

#### 7. Future extensibility

In a future iteration, the default constructibility requirement (rule #3) **may** be relaxed by allowing developers to register a factory function on the service builder. This factory function would be called by the framework whenever it needs to instantiate the custom validation exception, providing access to:

- The operation shape information
- The request context
- The specific constraint violations that occurred

```rust
// Future API (not part of this RFC)
let service = MyService::builder()
    .validation_exception_factory(|operation, request, violations| {
        CustomValidationException {
            message: format!("Validation failed for operation {}", operation.name()),
            error_code: determine_error_code(&violations),
            field_errors: map_violations_to_fields(violations),
        }
    })
    .build();
```

### Backwards compatibility

This feature is entirely opt-in and maintains full backward compatibility:

**Non-breaking changes:**
- Existing services using `smithy.framework#ValidationException` continue to work unchanged
- No changes to existing APIs or behavior for services without custom validation exception traits

**Breaking changes:**
- **Adding custom validation exception traits to an existing service is a breaking change** for the API contract
- Clients expecting the standard `ValidationException` format will receive the new custom format
- Service teams must coordinate with client teams when migrating to custom validation exceptions

How to actually implement this RFC
----------------------------------

### 1. Create validation exception traits

**Location**: `codegen-server-traits/src/main/resources/META-INF/smithy/validation-exception.smithy`

Define the new traits in the smithy-rs server traits namespace:

```smithy
$version: "2.0"

namespace smithy.rust.codegen.server.traits

/// Marks a structure as a custom validation exception that can replace
/// smithy.framework#ValidationException in operation error lists.
@trait(selector: "structure[trait|error]")
structure validationException {}

/// Marks a String member as the primary message field for a validation exception.
/// Exactly one member in a @validationException structure must have this trait.
@trait(selector: "structure[trait|smithy.rust.codegen.server.traits#validationException] > member[target=smithy.api#String]")
structure validationMessage {}

/// Marks a member as containing the list of field-level validation errors.
/// The target shape must be a String, List<String>, or List<Structure> where
/// the structure contains validation field information.
@trait(selector: "structure[trait|smithy.rust.codegen.server.traits#validationException] > member")
structure validationFieldList {}

/// Marks a String member as containing the field name in a validation field structure.
@trait(selector: "structure > member[target=smithy.api#String]")
structure validationFieldName {}

/// Marks a String member as containing the field error message in a validation field structure.
@trait(selector: "structure > member[target=smithy.api#String]")
structure validationFieldMessage {}
```

### 2. Validation logic

**Location**: `codegen-server/src/main/kotlin/software/amazon/smithy/rust/codegen/server/smithy/validators/`
Add validation to ensure custom validation exceptions are properly defined:

```kotlin
class CustomValidationExceptionValidator : Validator { override fun validate(model: Model): List<ValidationEvent> {
        val events = mutableListOf<ValidationEvent>()
        
        model.shapes(StructureShape::class.java)
            .filter { it.hasTrait(ValidationExceptionTrait::class.java) }
            .forEach { shape ->
                // Validate that the shape also has @error trait
                if (!shape.hasTrait(ErrorTrait::class.java)) {
                    events.add(ValidationEvent.builder()
                        .id("CustomValidationException.MissingErrorTrait")
                        .severity(Severity.ERROR)
                        .shape(shape)
                        .message("@validationException requires @error trait")
                        .build())
                }
                
                // Validate exactly one @validationMessage field
                val messageFields = shape.members().values
                    .filter { it.hasTrait(ValidationMessageTrait::class.java) }
                
                when (messageFields.size) {
                    0 -> events.add(ValidationEvent.builder()
                        .id("CustomValidationException.MissingMessageField")
                        .severity(Severity.ERROR)
                        .shape(shape)
                        .message("@validationException requires exactly one @validationMessage field")
                        .build())
                    1 -> { /* Valid */ }
                    else -> events.add(ValidationEvent.builder()
                        .id("CustomValidationException.MultipleMessageFields")
                        .severity(Severity.ERROR)
                        .shape(shape)
                        .message("@validationException can have only one @validationMessage field")
                        .build())
                }
                
                // Validate default constructibility
                validateDefaultConstructibility(shape, model, events)
            }
        
        return events
    }
}
```

### 3. Generated Rust Code Changes

#### 3.1 Structure of Validation Exceptions

The `ValidationExceptionField` structure will remain unchanged as it's independent of the modeled `ValidationException` shape:

```rust
pub struct ValidationExceptionField {
    pub path: ::std::string::String,
    pub message: ::std::string::String,
}
```

#### 3.2 From Constraint Violations to Custom Exceptions

**Step 1**: Each operation input has an associated `ConstraintViolation` enum representing possible validation failures:

```rust
pub mod get_storage_input {
    pub enum ConstraintViolation {
        /// `user` was not provided but it is required when building `GetStorageInput`.
        MissingUser,
        /// `passcode` was not provided but it is required when building `GetStorageInput`.
        MissingPasscode,
    }
}
```

**Step 2**: Each `ConstraintViolation` can be converted to a `ValidationExceptionField`:

```rust
impl ConstraintViolation {
    pub(crate) fn as_validation_exception_field(
        self,
        path: ::std::string::String,
    ) -> crate::model::ValidationExceptionField {
        match self {
            ConstraintViolation::MissingUser => crate::model::ValidationExceptionField {
                message: format!("Value at '{}/user' failed to satisfy constraint: Member must not be null", path),
                path: path + "/user",
            },
            ConstraintViolation::MissingPasscode => crate::model::ValidationExceptionField {
                message: format!("Value at '{}/passcode' failed to satisfy constraint: Member must not be null", path),
                path: path + "/passcode",
            },
        }
    }
}
```

**Step 3**: The `From<ConstraintViolation>` implementation for `RequestRejection` needs modification to use the custom exception:

```rust
impl From<ConstraintViolation> for RequestRejection {
    fn from(constraint_violation: ConstraintViolation) -> Self {
        // Convert the constraint violation to a ValidationExceptionField
        let field = constraint_violation.as_validation_exception_field("".to_owned());
        
        // CHANGE: Create CustomValidationException instead of ValidationException
        let custom_exception = crate::error::CustomValidationException::builder()
            .message(format!("1 validation error detected. {}", &field.message))
            .field_list(Some(vec![field]))
            .build();
        
        Self::ConstraintViolation(
            // CHANGE: Call serializer for CustomValidationException
            crate::protocol_serde::shape_custom_validation_exception::ser_custom_validation_exception_error(&custom_exception)
                .expect("serialization should not fail")
        )
    }
}
```

#### 3.3 Serialization Function

A new serialization function must be implemented for the custom exception:

```rust
// CHANGE: New serialization function for CustomValidationException
pub fn ser_custom_validation_exception(
    object: &mut JsonObjectWriter,
    input: &crate::error::CustomValidationException,
) -> Result<(), SerializationError> {
    // Serialize standard fields
    object.key("message").string(&input.message);
    
    // Serialize validation field list
    if let Some(fields) = &input.field_list {
        let mut array = object.key("fieldList").array();
        for item in fields {
            let mut obj = array.object();
            crate::protocol_serde::shape_validation_exception_field::ser_validation_exception_field(&mut obj, item)?;
            obj.finish();
        }
        array.finish();
    }
    
    // Serialize any custom fields
    // [serialization code for custom fields]
    
    Ok(())
}
```

### 4. Code Generator Changes

**Location**: `software/amazon/smithy/rust/codegen/server/smithy/customizations/CustomValidationGeneratorDecorator.kt`

To support custom validation exceptions, we need to create a new decorator that follows a similar pattern to the existing `SmithyValidationExceptionDecorator`. The key changes are:

```kotlin
class CustomValidationExceptionDecorator : ServerCodegenDecorator {
    override val name: String
        get() = "CustomValidationExceptionDecorator"
    override val order: Byte
        get() = 69
    override fun validationExceptionConversion(
        codegenContext: ServerCodegenContext,
    ): ValidationExceptionConversionGenerator = CustomValidationExceptionConversionGenerator(codegenContext)
}
```

The existing validation field generation logic should be refactored into a common class:

```kotlin
// New shared utility class
class ValidationExceptionFieldGenerator(private val codegenContext: ServerCodegenContext) {
    // Common code for generating ValidationExceptionField structures and conversion methods
    fun generateValidationExceptionField(): Writable {
        // Implementation moved from SmithyValidationExceptionDecorator
    }
}
```

#### Define a Builder for the `CustomValidationException`

Unlike Smithy's standard `ValidationException` which doesn't have a builder, we should generate a builder for the `CustomValidationException` to simplify its construction and allow for default values:

```kotlin
fun generateCustomValidationExceptionBuilder(): Writable {
    return writer {
        write("""
            impl ${codegenContext.customExceptionName} {
                /// Create a new builder for the custom validation exception
                pub fn builder() -> ${codegenContext.customExceptionName}Builder {
                    ${codegenContext.customExceptionName}Builder::default()
                }
            }
            
            /// Builder for ${codegenContext.customExceptionName}
            #[derive(Default)]
            pub struct ${codegenContext.customExceptionName}Builder {
                message: Option<String>,
                field_list: Option<Vec<ValidationExceptionField>>,
                // Add additional fields from the custom exception model
                ${renderAdditionalBuilderFields()}
            }
            
            impl ${codegenContext.customExceptionName}Builder {
                /// Set the error message
                pub fn message(mut self, message: impl Into<String>) -> Self {
                    self.message = Some(message.into());
                    self
                }
                
                /// Set the list of validation exception fields
                pub fn field_list(mut self, field_list: Vec<ValidationExceptionField>) -> Self {
                    self.field_list = Some(field_list);
                    self
                }
                
                ${renderAdditionalBuilderMethods()}
                
                /// Build the custom validation exception
                pub fn build(self) -> Result<${codegenContext.customExceptionName}, String> {
                    let message = self.message.ok_or("message is required")?;
                    
                    Ok(${codegenContext.customExceptionName} {
                        message,
                        field_list: self.field_list,
                        ${renderAdditionalBuildFields()}
                    })
                }
            }
        """.trimIndent())
    }
}
```

Then, the custom validation exception generator would implement the conversion from constraint violations to the custom exception:

```kotlin
class CustomValidationExceptionConversionGenerator(private val codegenContext: ServerCodegenContext) :
    ValidationExceptionConversionGenerator {
    
    override fun renderImplFromConstraintViolationForRequestRejection(protocol: ServerProtocol): Writable {
        return writer {
            write("""
                impl #{From}<ConstraintViolation> for #{RequestRejection} {
                    fn from(constraint_violation: ConstraintViolation) -> Self {
                        let first_validation_exception_field =
                            constraint_violation.as_validation_exception_field("".to_owned());
                        
                        // Create custom validation exception using the builder
                        let custom_exception = crate::error::${codegenContext.customExceptionName}::builder()
                            .message(format!("1 validation error detected. {}", &first_validation_exception_field.message))
                            .field_list(Some(vec![first_validation_exception_field]))
                            .build()
                            .expect("Custom validation exception should be valid");
                        
                        Self::ConstraintViolation(
                            crate::protocol_serde::shape_${codegenContext.customExceptionName.decapitalize()}::ser_${codegenContext.customExceptionName.decapitalize()}_error(&custom_exception)
                                .expect("validation exceptions should never fail to serialize")
                        )
                    }
                }
            """.trimIndent())
        }
    }
}
```

### 5. Testing strategy

Comprehensive testing is required to ensure the feature works correctly:

1. **Trait validation tests**: Ensure custom validation exception traits are properly validated
2. **Code generation tests**: Verify correct Rust code is generated for various custom validation exception configurations
3. **Integration tests**: Test end-to-end validation exception handling with custom exceptions
4. **Backward compatibility tests**: Ensure existing services continue to work unchanged
5. **Error message tests**: Verify custom validation exceptions contain expected field information

Changes Checklist
-----------------
- [ ] Create `validationException`, `validationMessage`, `validationFieldList`, `validationFieldName`, and `validationFieldMessage` traits in `codegen-server-traits`
- [ ] Implement `CustomValidationExceptionValidator` to validate proper usage of custom validation exception traits
- [ ] Create shared `ValidationExceptionFieldGenerator` for common validation field generation logic
- [ ] Implement `CustomValidationExceptionDecorator` and `CustomValidationExceptionConversionGenerator` to generate custom exception mapping logic
- [ ] Add builder pattern generation for custom validation exception shapes via `CustomValidationExceptionBuilderGenerator`
- [ ] Update `From<ConstraintViolation>` implementations to create custom exceptions instead of `ValidationException`
- [ ] Implement serialization functions for custom validation exception shapes
- [ ] Add comprehensive unit tests for trait validation logic
- [ ] Add integration tests for end-to-end custom validation exception handling
- [ ] Create documentation explaining custom validation exception usage and migration strategies
- [ ] Add examples showing various custom validation exception patterns
- [ ] Update existing constraint violation documentation to mention custom validation exceptions
- [ ] Ensure backward compatibility with existing services using standard `ValidationException`