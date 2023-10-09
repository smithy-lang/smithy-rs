/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#![allow(dead_code)]

#[cfg(target_family = "wasm")]
mod default_config;
#[cfg(target_family = "wasm")]
mod http;
#[cfg(target_family = "wasm")]
mod list_objects;
#[cfg(all(target_family = "wasm", target_os = "wasi"))]
mod wasi;
