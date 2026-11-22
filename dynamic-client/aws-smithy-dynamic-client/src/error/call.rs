/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_lang::ShapeId;
use aws_smithy_schema::Schema;
use std::error::Error;
use std::fmt;

/// The error of a call to an operation the service does not have.
///
/// A call fails with this error as the source of an `SdkError::ConstructionFailure`, and sends
/// no request.
#[derive(Debug)]
pub struct OperationNotFound {
    operation: String,
    service: ShapeId,
}

impl OperationNotFound {
    pub(crate) fn new(operation: String, service: ShapeId) -> Self {
        Self { operation, service }
    }

    /// Returns the operation name of the call.
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Returns the shape ID of the service.
    pub fn service(&self) -> &ShapeId {
        &self.service
    }
}

impl fmt::Display for OperationNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "service `{}` has no operation `{}`",
            self.service, self.operation
        )
    }
}

impl Error for OperationNotFound {}

/// The error of an input value that does not match its schema.
///
/// A call fails with this error as the source of an `SdkError::ConstructionFailure`, and sends
/// no request. [`DynamicClient::struct_value`](crate::DynamicClient::struct_value) returns this
/// error too.
#[derive(Debug)]
pub struct InputError {
    path: String,
    shape: String,
    kind: InputErrorKind,
}

/// Why an input value does not match its schema.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputErrorKind {
    /// The value has a different type from its schema, such as a number for a string.
    WrongType,
    /// The number does not fit its integer type, or is not a whole number.
    InvalidNumber,
    /// A structure has a member its schema does not have.
    UnknownMember,
    /// A union value does not have exactly one member.
    InvalidUnion,
    /// The schema's type has no document value, such as a blob or a timestamp.
    Unsupported,
    /// The service reaches no shape with the given shape ID.
    UnknownShape,
    /// The shape is not a structure or a union.
    NotAStructureOrUnion,
}

impl InputError {
    // An error about the value of `schema`. The path is empty until `under` adds segments.
    pub(crate) fn new(schema: &Schema<'_>, kind: InputErrorKind) -> Self {
        Self::for_shape(schema.shape_id().as_str().to_owned(), kind)
    }

    // An error about the whole value of the shape `shape`.
    pub(crate) fn for_shape(shape: String, kind: InputErrorKind) -> Self {
        Self {
            path: String::new(),
            shape,
            kind,
        }
    }

    // Puts `segment`, a member name, map key, or list index, in front of the error's path.
    pub(crate) fn under(mut self, segment: &str) -> Self {
        let segment = segment.replace('~', "~0").replace('/', "~1");
        self.path.insert_str(0, &segment);
        self.path.insert(0, '/');
        self
    }

    /// Returns the JSON pointer of the value in the input, such as `/tags/0/key`. The pointer of
    /// the whole input is empty.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns why the value does not match its schema.
    pub fn kind(&self) -> &InputErrorKind {
        &self.kind
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = if self.path.is_empty() {
            "/"
        } else {
            &self.path
        };
        let problem = match self.kind {
            InputErrorKind::WrongType => "has the wrong type for",
            InputErrorKind::InvalidNumber => "is not a valid number for",
            InputErrorKind::UnknownMember => "is not a member of",
            InputErrorKind::InvalidUnion => "must have exactly one member of",
            InputErrorKind::Unsupported => "can't be given as a document for",
            InputErrorKind::UnknownShape => "is for a shape the service doesn't reach:",
            InputErrorKind::NotAStructureOrUnion => {
                "is for a shape that isn't a structure or union:"
            }
        };
        write!(f, "the input value at `{path}` {problem} `{}`", self.shape)
    }
}

impl Error for InputError {}
