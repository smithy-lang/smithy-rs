/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Collection of modules that get conditionally included directly into the code generated
//! SDK service crates. For example, when generating S3, the `s3_errors` module will get copied
//! into the generated S3 crate to support the code generator.
//!
//! This is _NOT_ intended to be an actual crate. It is a cargo project to solely to aid
//! with local development of the SDK.

#![allow(clippy::derive_partial_eq_without_eq)]
#![warn(
    missing_docs,
    rustdoc::missing_crate_level_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub
)]

/// Interceptors for API Gateway
pub mod apigateway_interceptors;

/// Stub credentials provider for use when no credentials provider is used.
pub mod no_credentials;

/// Support types required for adding presigning to an operation in a generated service.
pub mod presigning;

/// Presigning tower service
pub mod presigning_service;

/// Presigning interceptors
pub mod presigning_interceptors;

/// Special logic for extracting request IDs from S3's responses.
pub mod s3_request_id;

/// Glacier-specific checksumming behavior
pub mod glacier_checksums;

/// Glacier-specific behavior
pub mod glacier_interceptors;

/// Default middleware stack for AWS services
pub mod middleware;

/// Strip prefixes from IDs returned by Route53 operations when those IDs are used to construct requests
pub mod route53_resource_id_preprocessor_middleware;

/// Strip prefixes from IDs returned by Route53 operations when those IDs are used to construct requests
pub mod route53_resource_id_preprocessor;

pub mod http_request_checksum;
pub mod http_response_checksum;

// TODO(enableNewSmithyRuntimeCleanup): Delete this module
/// Convert a streaming `SdkBody` into an aws-chunked streaming body with checksum trailers
pub mod http_body_checksum_middleware;

#[allow(dead_code)]
pub mod endpoint_discovery;

// This module is symlinked in from the smithy-rs rust-runtime inlineables so that
// the `presigning_interceptors` module can refer to it.
mod serialization_settings;

// just so docs work
#[allow(dead_code)]
/// allow docs to work
#[derive(Debug)]
pub struct Client;
