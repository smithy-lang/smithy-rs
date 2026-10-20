/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Static facts about the standard and AWS endpoint functions: what the compiler needs to
//! know, never how to evaluate them.

use crate::endpoint_rules::language::{Literal, Template};

/// A function's declared return type, as far as the compiler distinguishes them. Mirrors
/// Smithy Java's `Type`, which the ported transforms compare against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReturnType {
    Boolean,
    String,
    StringArray,
    Record,
    /// Depends on the arguments (`coalesce`, `ite`, `getAttr`).
    Any,
    /// A value that may be absent, such as `substring`'s.
    Optional(&'static ReturnType),
}

impl ReturnType {
    /// The type without its optional wrapper, as Java's coalescing unwraps it.
    pub(super) fn inner(self) -> ReturnType {
        match self {
            ReturnType::Optional(inner) => *inner,
            other => other,
        }
    }

    /// The value a coalesced binding falls back to, if the type has one. Like Java's
    /// `OptionalType`, an optional type has its inner type's zero value. Records have none.
    pub(super) fn zero(self) -> Option<Literal> {
        match self {
            ReturnType::Boolean => Some(Literal::Boolean(false)),
            ReturnType::String => Some(Literal::String(Template::default())),
            ReturnType::StringArray => Some(Literal::Tuple(Box::default())),
            ReturnType::Optional(inner) => inner.zero(),
            ReturnType::Record | ReturnType::Any => None,
        }
    }
}

/// A known endpoint function.
#[derive(Debug)]
pub(super) struct Function {
    pub(super) name: &'static str,
    pub(super) returns: ReturnType,
}

const fn function(name: &'static str, returns: ReturnType) -> Function {
    Function { name, returns }
}

use ReturnType::{Any, Boolean, Optional, Record, String, StringArray};

/// The functions of rules engine 1.1 and the AWS extensions, as defined by Smithy Java.
const FUNCTIONS: &[Function] = &[
    function("booleanEquals", Boolean),
    function("coalesce", Any),
    function("getAttr", Any),
    function("isSet", Boolean),
    function("isValidHostLabel", Boolean),
    function("ite", Any),
    function("not", Boolean),
    function("parseURL", Optional(&Record)),
    function("split", StringArray),
    function("stringEquals", Boolean),
    function("substring", Optional(&String)),
    function("uriEncode", String),
    function("aws.isVirtualHostableS3Bucket", Boolean),
    function("aws.parseArn", Optional(&Record)),
    function("aws.partition", Optional(&Record)),
];

/// The function named `name`.
pub(super) fn get(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name == name)
}
