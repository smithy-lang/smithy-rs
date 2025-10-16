# Validation Exceptions

## Terminology

- **Constrained shape**: a shape that is either:
  - a shape with a [constraint trait](https://smithy.io/2.0/spec/constraint-traits.html) attached
  - a (member) shape with a [`required` trait](https://smithy.io/2.0/spec/type-refinement-traits.html#required-trait) attached
  - an [`enum`](https://smithy.io/2.0/spec/simple-types.html#enum) shape
  - an [`intEnum`](https://smithy.io/2.0/spec/simple-types.html#intenum) shape
  - a [`structure shape`](https://smithy.io/2.0/spec/aggregate-types.html#structure) with at least one required member shape; or
  - a shape whose closure includes any of the above.
- **ValidationException**: A Smithy error shape that is serialized in the response when constraint validation fails during request processing.
- **Shape closure**: the set of shapes a shape can "reach", including itself.
- **Custom validation exception**: A user-defined error shape marked with validation-specific traits that replaces the standard smithy.framework#ValidationException.

If an operation takes an input that is constrained, it can fail with a validation exception.
In these cases, you must model this behavior in the operation shape in your model file.

In the example below, the `GetCity` operation takes a required `cityId`. This means it is a constrained shape, so the validation exception behavior must be modeled.
As such, attempting to build this model will result in a codegen exception explaining this because.

```smithy
$version: "2"

namespace example.citylocator

use aws.protocols#awsJson1_0

@awsJson1_0
service CityLocator {
  version: "2006-03-01"
  resources: [
    City
  ]
}

resource City {
  identifiers: {
    cityId: CityId
  }
  properties: {
    coordinates: CityCoordinates
  }
  read: GetCity
}

@pattern("^[A-Za-z0-9 ]+$")
string CityId

structure CityCoordinates {
  @required
  latitude: Float

  @required
  longitude: Float
}

@readonly
operation GetCity {
  input := for City {
    // "cityId" provides the identifier for the resource and
    // has to be marked as required.
    @required
    $cityId
  }

  output := for City {
    // "required" is used on output to indicate if the service
    // will always provide a value for the member.
    // "notProperty" indicates that top-level input member "name"
    // is not bound to any resource property.
    @required
    @notProperty
    name: String

    @required
    $coordinates
  }

  errors: [
    NoSuchResource
  ]
}

// "error" is a trait that is used to specialize
// a structure as an error.
@error("client")
structure NoSuchResource {
  @required
  resourceType: String
}
```

## Default validation exception

The typical way forward is to use Smithy's default validation exception.

This can go per operation error closure, or in the service's error closure to apply to all operations.

e.g.

```smithy
use smithy.framework#ValidationException

...
operation GetCity {
  ...
  errors: [
    ...
    ValidationException
  ]
}
```

## Custom validation exception

In certain cases, you may want to define a custom validation exception. Some reasons for this could be:

- **Backward compatibility**: Migrating existing APIs to Smithy with a requirement of maintaining the existing validation exception format
- **Published APIs**: Already published a Smithy model with validation exception schemas to external consumers and cannot change the response format without breaking clients
- **Custom error handling**: General needs for additional fields or different field names for validation errors

The following five traits are provided for defining custom validation exceptions.

- @validationException
- @validationMessage
- @validationFieldList
- @validationFieldName
- @validationFieldMessage

### User guide

#### Requirements

**1. Define a custom validation exception shape**

Define a custom validation exception by applying the `@validationException` trait to any structure shape that is also marked with the `@error` trait.
```smithy
@validationException
@error("client")
structure CustomValidationException {
    // Structure members defined below
}
```

**2. Specify the message field (required)**

The custom validation exception **must** have **exactly one** String member marked with the `@validationMessage` trait to serve as the primary error message.
```smithy
use smithy.framework.rust#validationException

@validationException
@error("client")
structure CustomValidationException {
  @validationMessage
  @required
  message: String

  // <... other fields ...>
}
```

**3. Default constructibility requirement**
The custom validation exception structure **must** be default constructible. This means the shape either:

1. **Must not** contain any constrained shapes that the framework cannot construct; or
1. Any constrained shapes **must** have default values specified

For example, if we have `errorKind` enum member, we must specify the default with `@default()`. Otherwise, the
model will fail to build.
```smithy
@validationException
@error("client")
structure CustomValidationException {
  @validationMessage
  @required
  message: String,

  @default("errorInValidation") <------- must be specified
  errorKind: ErrorKind
}

enum ErrorKind {
  ERROR_IN_VALIDATION = "errorInValidation",
  SOME_OTHER_ERROR = "someOtherError",
}
```

**4. Optional Field List Support**

Optionally, the custom validation exception **may** include a field marked with `@validationFieldList` to provide detailed information about which fields failed validation.
This **must** be a list shape where the member is a structure shape with detailed field information:

- **Must** have a String member marked with `@validationFieldName`
- **May** have a String member marked with `@validationFieldMessage`
- Regarding additional fields:
  - The structure may have no additional fields beyond those specified above, or
  - If additional fields are present, each must be default constructible

```smithy
@validationException
@error("client")
structure CustomValidationException {
  @validationMessage
  @required
  message: String,

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

**5. Using the custom validation exception in operations**

```smithy
operation GetCity {
  ...
  errors: [
    ...
    CustomValidationException
  ]
}
```

### Limitations

It is unsupported to do the following and will result in an error if modeled:

- Defining multiple custom validation exceptions
- Including the default Smithy validation exception in an error closure if a custom validation exception is defined
