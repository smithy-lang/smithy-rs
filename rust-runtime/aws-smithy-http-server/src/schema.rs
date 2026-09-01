/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Server-owned schema descriptors.

use aws_smithy_schema::{Schema, ShapeId};

use crate::routing::PrefixPolicy;

/// Runtime descriptor for a Smithy service.
#[derive(Debug)]
pub struct ServiceSchema<'a> {
    schema: &'a Schema<'a>,
    version: Option<&'a str>,
    protocols: &'a [ShapeId<'a>],
    operations: &'a [&'a OperationSchema<'a>],
}

impl<'a> ServiceSchema<'a> {
    /// Creates a service descriptor from generated schema metadata.
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

    /// Returns the Smithy service version, if modeled.
    pub fn version(&self) -> Option<&'a str> {
        self.version
    }

    /// Returns the protocol shape IDs modeled on this service.
    pub fn protocols(&self) -> &'a [ShapeId<'a>] {
        self.protocols
    }

    /// Returns operation descriptors for operations bound to this service.
    pub fn operations(&self) -> &'a [&'a OperationSchema<'a>] {
        self.operations
    }
}

/// Runtime descriptor for a Smithy operation.
#[derive(Debug)]
pub struct OperationSchema<'a> {
    schema: &'a Schema<'a>,
    input: &'a Schema<'a>,
    output: &'a Schema<'a>,
    errors: &'a [&'a Schema<'a>],
    prefix_policy: PrefixPolicy,
}

impl<'a> OperationSchema<'a> {
    /// Creates an operation descriptor from generated schema metadata.
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
            prefix_policy: PrefixPolicy::DEFAULT,
        }
    }

    /// Sets route prefix policy metadata for this operation descriptor.
    pub const fn with_prefix_policy(mut self, prefix_policy: PrefixPolicy) -> Self {
        self.prefix_policy = prefix_policy;
        self
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

    /// Returns schemas for errors modeled on this operation.
    pub fn errors(&self) -> &'a [&'a Schema<'a>] {
        self.errors
    }

    /// Returns route prefix policy metadata for this operation.
    pub fn prefix_policy(&self) -> PrefixPolicy {
        self.prefix_policy
    }
}
