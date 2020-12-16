/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub mod instant;
use std::collections::HashMap;

pub use crate::instant::Instant;

#[derive(Debug, PartialEq, Clone)]
pub struct Blob {
    inner: Vec<u8>,
}

impl Blob {
    pub fn new<T: Into<Vec<u8>>>(inp: T) -> Self {
        Blob { inner: inp.into() }
    }
}

impl AsRef<[u8]> for Blob {
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Document {
    Object(HashMap<String, Document>),
    Array(Vec<Document>),
    Number(Number),
    String(String),
    Bool(bool),
    Null,
}

/// A number type that implements Javascript / JSON semantics, modeled on serde_json:
/// https://docs.serde.rs/src/serde_json/number.rs.html#20-22
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}
