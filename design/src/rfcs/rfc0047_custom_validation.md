RFC: Custom Validation Exception 
================================

> Status: RFC
>
> Applies to: server

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

This RFC defines a mechanism to use custom `ValidationException` instead of `smithy.framework#ValidationException`, enabling service teams to use a validation exception that they might have already published to the external world or maybe they are porting an existing non-smithy based model to smithy and need backward compatibility. 

Terminology
-----------

- **Constrained shape**: a shape that is either:
1. a shape with a [constraint trait][https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html] attached
2. a (member) shape with a [required trait] attached
3. an [enum shape]
4. an [intEnum shape]
5. a [structure shape] with at least one required member shape; or
6. a shape whose closure includes any of the above.
- **ValidationException**: A Smithy shape that should be serialized on the wire in case the required field is not present.
- **Shape closure**:
- **::std::hash::Hash**:

The user experience if this RFC is implemented
----------------------------------------------

If there is a constrained shape in the input shape closure of an operation, then the operation must add a `ValidationException` to the operation' error list, otherwise an error is raised.

```
Caused by: ValidationResult(shouldAbort=true, messages=[LogMessage(level=SEVERE, message=Operation com.aws.example#GetStorage takes in input that is constrained (https://awslabs.github.io/smithy/2.0/spec/constraint-traits.html), and as such can fail with a validation exception. You must model this behavior in the operation shape in your model file.
```smithy
use smithy.framework#ValidationException

operation GetStorage {
    ...
    errors: [..., ValidationException] // <-- Add this.
}
```)])
```

The solution is to add the `ValidationException` in the errors list:
```
operation SomeOperation {
    # <...input / output definiton..>
    errors: [
        # <...other fields...>
        ValidationException
    ]
}
```

Once this RF is implemented, service developers will be able to:

1. **Mark a shape to be used as ValidationException** by applying the `@validationException` trait to any structure shape that is marked with `@error` trait.

```smithy
@validationException
@error
structure CustomValidationException {
}
```

2. There **must** be a String member shape field that is marked as the **message field** using the `@validationMessage` trait.

3. For now, the structure must be default constructible. If it has any required shape those must be  

<!-- Explain the implementation of this new feature -->
How to actually implement this RFC
----------------------------------

In order to implement this feature, we need to add X and update Y...

<!-- Include a checklist of all the things that need to happen for this RFC's implementation to be considered complete -->
Changes checklist
-----------------

- [x] Create new struct `NewFeature`
