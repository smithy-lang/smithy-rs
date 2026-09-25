/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime descriptors for a service and its operations.
//!
//! [`Schema`] describes a single shape. Services and operations additionally
//! relate shapes to one another: an operation has an input, an output, and a
//! set of errors; a service has a version, a set of protocols, and a set of
//! operations. [`OperationSchema`] and [`ServiceSchema`] carry those
//! relationships as references to the shape schemas, so a generated
//! `&'static ServiceSchema<'static>` is a complete, read-only description of
//! the service the router serves.
//!
//! Operation-level traits are read from the shapes that carry them: codegen
//! records an operation's `@http` binding on its input schema (see
//! [`OperationSchema::http`]).
//!
//! Like [`Schema`], both descriptors are `const`-constructible and covariant
//! in `'a`, so codegen-emitted `'static` descriptors coerce to any shorter
//! lifetime at call sites.

use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{Schema, ShapeId};

/// Runtime descriptor for a Smithy operation shape.
#[derive(Debug)]
pub struct OperationSchema<'a> {
    shape_id: ShapeId<'a>,
    input: &'a Schema<'a>,
    output: &'a Schema<'a>,
    errors: &'a [&'a Schema<'a>],
    compat_name: Option<&'a str>,
}

impl<'a> OperationSchema<'a> {
    /// Creates an operation descriptor.
    pub const fn new(
        shape_id: ShapeId<'a>,
        input: &'a Schema<'a>,
        output: &'a Schema<'a>,
        errors: &'a [&'a Schema<'a>],
    ) -> Self {
        Self {
            shape_id,
            input,
            output,
            errors,
            compat_name: None,
        }
    }

    /// Sets the operation name that name-keyed routing matches in place of the
    /// modeled shape name. See [`Self::compat_name`].
    pub const fn with_compat_name(mut self, name: &'a str) -> Self {
        self.compat_name = Some(name);
        self
    }

    /// Returns the operation shape ID.
    pub fn shape_id(&self) -> &ShapeId<'a> {
        &self.shape_id
    }

    /// Returns the operation's `@http` binding, if it has one.
    ///
    /// Codegen records the binding on the operation's input schema, so this
    /// reads it from there.
    pub fn http(&self) -> Option<&'a HttpTrait<'a>> {
        self.input.http()
    }

    /// Returns the operation input shape schema.
    pub fn input(&self) -> &'a Schema<'a> {
        self.input
    }

    /// Returns the operation output shape schema.
    pub fn output(&self) -> &'a Schema<'a> {
        self.output
    }

    /// Returns the schemas of the errors modeled on this operation.
    pub fn errors(&self) -> &'a [&'a Schema<'a>] {
        self.errors
    }

    /// Returns the operation name that name-keyed routing matches in place of
    /// the modeled shape name, when one is set.
    ///
    /// Protocols that select operations by name (such as awsJson's
    /// `X-Amz-Target`) route on this name when present, falling back to
    /// [`shape_id`](Self::shape_id)'s shape name otherwise.
    pub fn compat_name(&self) -> Option<&'a str> {
        self.compat_name
    }
}

/// Runtime descriptor for a Smithy service shape.
#[derive(Debug)]
pub struct ServiceSchema<'a> {
    shape_id: ShapeId<'a>,
    version: Option<&'a str>,
    protocols: &'a [ShapeId<'a>],
    operations: &'a [&'a OperationSchema<'a>],
}

impl<'a> ServiceSchema<'a> {
    /// Creates a service descriptor.
    ///
    /// `protocols` lists the shape IDs of the protocols the service serves,
    /// and `operations` lists every operation bound to the service, including
    /// operations bound through resources.
    pub const fn new(
        shape_id: ShapeId<'a>,
        version: Option<&'a str>,
        protocols: &'a [ShapeId<'a>],
        operations: &'a [&'a OperationSchema<'a>],
    ) -> Self {
        Self {
            shape_id,
            version,
            protocols,
            operations,
        }
    }

    /// Returns the service shape ID.
    pub fn shape_id(&self) -> &ShapeId<'a> {
        &self.shape_id
    }

    /// Returns the modeled service version, if any.
    pub fn version(&self) -> Option<&'a str> {
        self.version
    }

    /// Returns the shape IDs of the protocols applied to this service.
    pub fn protocols(&self) -> &'a [ShapeId<'a>] {
        self.protocols
    }

    /// Returns the operations bound to this service.
    pub fn operations(&self) -> &'a [&'a OperationSchema<'a>] {
        self.operations
    }

    /// Returns the operation with the given shape ID, if bound to this service.
    pub fn operation(&self, id: &ShapeId<'_>) -> Option<&'a OperationSchema<'a>> {
        self.operations
            .iter()
            .copied()
            .find(|operation| operation.shape_id().as_str() == id.as_str())
    }
}

// Covariance in `'a` is what lets a codegen-emitted `&'static` descriptor be
// passed where a `&ServiceSchema<'_>` is expected.
#[allow(dead_code)]
fn _assert_operation_schema_covariant<'a, 'b: 'a>(s: OperationSchema<'b>) -> OperationSchema<'a> {
    s
}

#[allow(dead_code)]
fn _assert_service_schema_covariant<'a, 'b: 'a>(s: ServiceSchema<'b>) -> ServiceSchema<'a> {
    s
}

#[cfg(test)]
mod test {
    use super::*;
    use aws_smithy_schema::{shape_id, ShapeType};

    static UNIT: Schema<'static> = Schema::new(shape_id!("smithy.api", "Unit"), ShapeType::Structure);
    static GET_INPUT: Schema<'static> = Schema::new(shape_id!("example", "GetInput"), ShapeType::Structure)
        .with_http(HttpTrait::new("GET", "/get/{id}", None));
    static NOT_FOUND: Schema<'static> = Schema::new(shape_id!("example", "NotFound"), ShapeType::Structure);
    static GET_ERRORS: &[&Schema<'static>] = &[&NOT_FOUND];
    static GET: OperationSchema<'static> =
        OperationSchema::new(shape_id!("example", "Get"), &GET_INPUT, &UNIT, GET_ERRORS);
    static PUT: OperationSchema<'static> = OperationSchema::new(shape_id!("example", "Put"), &UNIT, &UNIT, &[]);
    static PROTOCOLS: &[ShapeId<'static>] = &[shape_id!("aws.protocols", "restJson1")];
    static OPERATIONS: &[&OperationSchema<'static>] = &[&GET, &PUT];
    static SERVICE: ServiceSchema<'static> =
        ServiceSchema::new(shape_id!("example", "Service"), Some("2024-01-01"), PROTOCOLS, OPERATIONS);

    #[test]
    fn operation_descriptor_exposes_shapes_and_http_binding() {
        assert_eq!(GET.shape_id().as_str(), "example#Get");
        assert!(std::ptr::eq(GET.input(), &GET_INPUT));
        assert!(std::ptr::eq(GET.output(), &UNIT));
        assert_eq!(GET.errors().len(), 1);
        assert_eq!(GET.errors()[0].shape_id().as_str(), "example#NotFound");

        let http = GET.http().expect("@http on the operation input");
        assert_eq!(http.method(), "GET");
        assert_eq!(http.uri(), "/get/{id}");
        assert!(PUT.http().is_none());
    }

    #[test]
    fn service_descriptor_exposes_version_protocols_and_operations() {
        assert_eq!(SERVICE.shape_id().as_str(), "example#Service");
        assert_eq!(SERVICE.version(), Some("2024-01-01"));
        assert_eq!(SERVICE.protocols().len(), 1);
        assert_eq!(SERVICE.protocols()[0].as_str(), "aws.protocols#restJson1");
        assert_eq!(SERVICE.operations().len(), 2);
    }

    #[test]
    fn service_descriptor_looks_up_operations_by_shape_id() {
        let put = SERVICE.operation(&shape_id!("example", "Put")).expect("bound operation");
        assert!(std::ptr::eq(put, &PUT));
        assert!(SERVICE.operation(&shape_id!("example", "Missing")).is_none());
    }

    #[test]
    fn descriptors_can_be_built_with_a_non_static_lifetime() {
        let operation = OperationSchema::new(ShapeId::from_parts("ns#Op", "ns", "Op"), &UNIT, &UNIT, &[]);
        let operations = [&operation];
        let service = ServiceSchema::new(ShapeId::from_parts("ns#Svc", "ns", "Svc"), None, &[], &operations);

        assert_eq!(service.operations()[0].shape_id().as_str(), "ns#Op");
        assert!(service.version().is_none());
    }
}
