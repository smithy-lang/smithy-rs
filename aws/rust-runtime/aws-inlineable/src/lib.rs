/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/* Automatically managed default lints */
#![cfg_attr(docsrs, feature(doc_cfg))]
/* End of automatically managed default lints */
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

/// Supporting code for the account based endpoints.
#[allow(dead_code)]
pub mod account_id_endpoint;

/// Supporting code for the aws-chunked content encoding.
pub mod aws_chunked;

/// Supporting code to determine auth scheme options based on the `authSchemes` endpoint list property.
#[allow(dead_code)]
pub mod endpoint_auth;

/// Interceptors for API Gateway
pub mod apigateway_interceptors;

/// Support types required for adding presigning to an operation in a generated service.
pub mod presigning;

/// Presigning interceptors
pub mod presigning_interceptors;

// This module uses module paths that assume the target crate to which it is copied, e.g.
// `crate::config::endpoint::Params`. If included into `aws-inlineable`, this module would
// fail to compile.
// pub mod s3_express;

/// Special logic for extracting request IDs from S3's responses.
#[allow(dead_code)]
pub mod s3_request_id;

/// Glacier-specific behavior
pub mod glacier_interceptors;

/// Strip prefixes from IDs returned by Route53 operations when those IDs are used to construct requests
pub mod route53_resource_id_preprocessor;

pub mod http_request_checksum;
pub mod http_response_checksum;

#[allow(dead_code)]
pub mod endpoint_discovery;

// This module is symlinked in from the smithy-rs rust-runtime inlineables so that
// the `presigning_interceptors` module can refer to it.
mod serialization_settings;

/// Parse the Expires and ExpiresString fields correctly
#[allow(dead_code)]
pub mod s3_expires_interceptor;

// just so docs work
#[allow(dead_code)]
/// allow docs to work
#[derive(Debug)]
pub struct Client;

pub mod dsql_auth_token;
pub mod rds_auth_token;
