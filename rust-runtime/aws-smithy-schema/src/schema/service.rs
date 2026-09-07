/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime descriptors for Smithy service and operation shapes.
//!
//! [`Schema`] describes a single shape. Services and operations additionally
//! relate shapes to one another: an operation has an input, an output, and a
//! set of errors; a service has a version, a set of protocols, and a set of
//! operations. [`OperationSchema`] and [`ServiceSchema`] carry those
//! relationships as references to the shape schemas, so a generated
//! `&'static ServiceSchema<'static>` is a complete, read-only description of
//! the modeled service.
//!
//! These descriptors carry model information only. Anything specific to a
//! particular runtime (routing state, configuration) belongs in that runtime.
//!
//! Like [`Schema`], both descriptors are `const`-constructible and covariant
//! in `'a`, so codegen-emitted `'static` descriptors coerce to any shorter
//! lifetime at call sites.

use crate::{Schema, ShapeId};

/// Runtime descriptor for a Smithy operation shape.
///
/// The operation's own [`Schema`] carries operation-level traits such as
/// `@http`; the input, output, and error schemas are the shapes referenced by
/// the operation.
#[derive(Debug)]
pub struct OperationSchema<'a> {
    schema: &'a Schema<'a>,
    input: &'a Schema<'a>,
    output: &'a Schema<'a>,
    errors: &'a [&'a Schema<'a>],
}

impl<'a> OperationSchema<'a> {
    /// Creates an operation descriptor.
    ///
    /// `schema` must have shape type [`ShapeType::Operation`](crate::ShapeType::Operation).
    pub const fn new(
        schema: &'a Schema<'a>,
        input: &'a Schema<'a>,
        output: &'a Schema<'a>,
        errors: &'a [&'a Schema<'a>],
    ) -> Self {
        Self {
            schema,
            input,
            output,
            errors,
        }
    }

    /// Returns the operation shape schema.
    pub fn schema(&self) -> &'a Schema<'a> {
        self.schema
    }

    /// Returns the operation shape ID.
    pub fn shape_id(&self) -> &'a ShapeId<'a> {
        self.schema.shape_id()
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
}

/// Runtime descriptor for a Smithy service shape.
#[derive(Debug)]
pub struct ServiceSchema<'a> {
    schema: &'a Schema<'a>,
    version: Option<&'a str>,
    protocols: &'a [ShapeId<'a>],
    operations: &'a [&'a OperationSchema<'a>],
}

impl<'a> ServiceSchema<'a> {
    /// Creates a service descriptor.
    ///
    /// `schema` must have shape type [`ShapeType::Service`](crate::ShapeType::Service).
    /// `protocols` lists the shape IDs of the protocol traits applied to the
    /// service, and `operations` lists every operation bound to the service,
    /// including operations bound through resources.
    pub const fn new(
        schema: &'a Schema<'a>,
        version: Option<&'a str>,
        protocols: &'a [ShapeId<'a>],
        operations: &'a [&'a OperationSchema<'a>],
    ) -> Self {
        Self {
            schema,
            version,
            protocols,
            operations,
        }
    }

    /// Returns the service shape schema.
    pub fn schema(&self) -> &'a Schema<'a> {
        self.schema
    }

    /// Returns the service shape ID.
    pub fn shape_id(&self) -> &'a ShapeId<'a> {
        self.schema.shape_id()
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
// passed where a `&ServiceSchema<'_>` is expected. See the crate-level
// "Variance" section.
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
    use crate::traits::HttpTrait;
    use crate::{shape_id, ShapeType};

    static UNIT: Schema<'static> =
        Schema::new(shape_id!("smithy.api", "Unit"), ShapeType::Structure);
    static NOT_FOUND: Schema<'static> =
        Schema::new(shape_id!("example", "NotFound"), ShapeType::Structure);
    static GET_ERRORS: &[&Schema<'static>] = &[&NOT_FOUND];
    static GET_SHAPE: Schema<'static> = Schema::new(
        shape_id!("example", "Get"),
        ShapeType::Operation,
    )
    .with_http(HttpTrait::new("GET", "/get/{id}", None));
    static GET: OperationSchema<'static> =
        OperationSchema::new(&GET_SHAPE, &UNIT, &UNIT, GET_ERRORS);
    static PUT_SHAPE: Schema<'static> =
        Schema::new(shape_id!("example", "Put"), ShapeType::Operation);
    static PUT: OperationSchema<'static> = OperationSchema::new(&PUT_SHAPE, &UNIT, &UNIT, &[]);
    static SERVICE_SHAPE: Schema<'static> =
        Schema::new(shape_id!("example", "Service"), ShapeType::Service);
    static PROTOCOLS: &[ShapeId<'static>] = &[shape_id!("aws.protocols", "restJson1")];
    static OPERATIONS: &[&OperationSchema<'static>] = &[&GET, &PUT];
    static SERVICE: ServiceSchema<'static> =
        ServiceSchema::new(&SERVICE_SHAPE, Some("2024-01-01"), PROTOCOLS, OPERATIONS);

    #[test]
    fn operation_descriptor_exposes_shapes_and_http_trait() {
        assert_eq!(GET.shape_id().as_str(), "example#Get");
        assert_eq!(GET.schema().shape_type(), ShapeType::Operation);
        assert!(std::ptr::eq(GET.input(), &UNIT));
        assert!(std::ptr::eq(GET.output(), &UNIT));
        assert_eq!(GET.errors().len(), 1);
        assert_eq!(GET.errors()[0].shape_id().as_str(), "example#NotFound");

        let http = GET.schema().http().expect("@http on operation shape");
        assert_eq!(http.method(), "GET");
        assert_eq!(http.uri(), "/get/{id}");
        assert!(PUT.schema().http().is_none());
    }

    #[test]
    fn service_descriptor_exposes_version_protocols_and_operations() {
        assert_eq!(SERVICE.shape_id().as_str(), "example#Service");
        assert_eq!(SERVICE.schema().shape_type(), ShapeType::Service);
        assert_eq!(SERVICE.version(), Some("2024-01-01"));
        assert_eq!(SERVICE.protocols().len(), 1);
        assert_eq!(SERVICE.protocols()[0].as_str(), "aws.protocols#restJson1");
        assert_eq!(SERVICE.operations().len(), 2);
    }

    #[test]
    fn service_descriptor_looks_up_operations_by_shape_id() {
        let put = SERVICE
            .operation(&shape_id!("example", "Put"))
            .expect("bound operation");
        assert!(std::ptr::eq(put, &PUT));
        assert!(SERVICE
            .operation(&shape_id!("example", "Missing"))
            .is_none());
    }

    #[test]
    fn descriptors_can_be_built_with_a_non_static_lifetime() {
        let id = ShapeId::from_parts("ns#Op", "ns", "Op");
        let shape = Schema::new(id, ShapeType::Operation);
        let operation = OperationSchema::new(&shape, &UNIT, &UNIT, &[]);
        let operations = [&operation];
        let service_shape = Schema::new(
            ShapeId::from_parts("ns#Svc", "ns", "Svc"),
            ShapeType::Service,
        );
        let service = ServiceSchema::new(&service_shape, None, &[], &operations);

        assert_eq!(service.operations()[0].shape_id().as_str(), "ns#Op");
        assert!(service.version().is_none());
    }
}
