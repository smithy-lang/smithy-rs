/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! This module defines types that describe timeouts that can be applied to various stages of the
//! Smithy networking stack.

mod api;
mod config;
mod error;
mod http;
mod tcp;
mod util;

pub use api::Api;
pub use config::Config;
pub use error::ConfigError;
pub use http::Http;
pub use tcp::Tcp;
pub use util::parse_str_as_timeout;
