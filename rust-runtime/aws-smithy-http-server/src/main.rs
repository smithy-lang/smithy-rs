use std::convert::TryFrom;

pub enum NiceStringValidationError {
    /// Validation error holding the number of Unicode code points found, when a value between `1` and
    /// `69` (inclusive) was expected.
    LengthViolation(usize),
}

pub struct NiceString(String);

impl NiceString {
    pub fn parse(inner: String) -> Result<Self, NiceStringValidationError> {
        Self::try_from(inner)
    }
}

impl TryFrom<String> for NiceString {
    type Error = NiceStringValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let num_code_points = value.chars().count();
        if 1 <= num_code_points && num_code_points <= 69 {
            Ok(Self(value))
        } else {
            Err(NiceStringValidationError::LengthViolation(num_code_points))
        }
    }
}

// This implementation is used when formatting the error in the 400 HTTP response body, among other
// places.
// The format of the error messages will not be configurable by service implementers and its format
// will be:
//
//     `<StructName>` validation error: <constraint> violation: <message>
//
// where:
//     * <StructName> is the name of the generated struct,
//     * <constraint> is the name of the violated Smithy constraint; and
//     * <message> is a templated message specific to the particular constraint instantiation (so
//       e.g. `@length(min: 1)` may have a different template than `@length(min: 1, max: 69)`.
//
// If the shape is marked with `@sensitive`, the format of the message will be:
//
//     `<StructName` validation error
impl std::fmt::Display for NiceStringValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NiceStringValidationError::LengthViolation(found) => write!(
                f,
                "MyStruct validation error: length violation: expected value between 1 and 69, but found {}",
                found
            ),
        }
    }
}

// This is the `Debug` implementation if the shape is marked with `@sensitive`.
// If the shape is not marked with `@sensitive`, we will just `#[derive(Debug)]`.
impl std::fmt::Debug for NiceStringValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut formatter = f.debug_struct("NiceStringValidationError");
        formatter.finish()
    }
}

impl std::error::Error for NiceStringValidationError {}

// pub(crate) struct MyStructValidationErrorWrapper(MyStructValidationError);

// impl axum_core::response::IntoResponse for MyStructValidationErrorWrapper {
//     fn into_response(self) -> axum_core::response::Response {
//         let err = MyStructValidationError::LengthViolation(70);
//         let rejection = aws_smithy_http_server::rejection::SmithyRejection::ConstraintViolation(
//             aws_smithy_http_server::rejection::ConstraintViolation::from_err(err),
//         );
//         rejection.into_response()
//     }
// }

pub fn main() {
    let err = NiceStringValidationError::LengthViolation(70);
    let constraint_rejection = aws_smithy_http_server::rejection::ConstraintViolation::from_err(err);
    let deserialize_rejection = aws_smithy_http_server::rejection::Deserialize::from_err(constraint_rejection);
    let smithy_rejection = aws_smithy_http_server::rejection::SmithyRejection::Deserialize(deserialize_rejection);
}
