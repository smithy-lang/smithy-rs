/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use rustdoc_types::{Item, Span};
use std::fmt;

/// In a [`ContextStack`], the `ContextType` identifies what kind of thing each name in the
/// stack represents.
///
/// For example, if there was an enum `Foo` inside module `bar`, then a context stack would
/// roughly look like the following:
/// ```
/// [(ContextType::Module, "bar"), (ContextType::Enum, "Foo")]
/// ```
#[derive(Copy, Clone, Debug)]
pub enum ContextType {
    AssocType,
    Constant,
    Crate,
    Enum,
    EnumVariant,
    Function,
    Method,
    Module,
    ReExport,
    Static,
    Struct,
    StructField,
    Trait,
    TypeDef,
}

/// Represents one piece of context in a [`ContextStack`].
#[derive(Clone, Debug)]
struct Context {
    typ: ContextType,
    name: String,
    span: Option<Span>,
}

impl Context {
    fn new(typ: ContextType, name: String, span: Option<Span>) -> Context {
        Context { typ, name, span }
    }
}

/// Represents the full context to an item being visited by [`Visitor`](crate::visitor::Visitor).
///
/// This is equivalent to the type path of that item, which has to be re-assembled since
/// it is lost in the flat structure of the Rustdoc JSON output.
#[derive(Clone, Debug)]
pub struct ContextStack {
    stack: Vec<Context>,
}

impl ContextStack {
    pub fn new(crate_name: &str) -> Self {
        ContextStack {
            stack: vec![Context::new(ContextType::Crate, crate_name.into(), None)],
        }
    }

    pub fn push(&mut self, typ: ContextType, item: &Item) {
        self.push_raw(typ, item.name.as_ref().expect("name"), item.span.as_ref());
    }

    pub fn push_raw(&mut self, typ: ContextType, name: &str, span: Option<&Span>) {
        self.stack
            .push(Context::new(typ, name.into(), span.cloned()));
    }

    /// Returns the span (file + beginning and end positions) of the last [`Context`] in the stack.
    pub fn last_span(&self) -> Option<&Span> {
        self.stack.last().map(|c| c.span.as_ref()).flatten()
    }

    /// Returns the [`ContextType`] of the last [`Context`] in the stack.
    pub fn last_typ(&self) -> Option<ContextType> {
        self.stack.last().map(|c| c.typ)
    }
}

impl fmt::Display for ContextStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self
            .stack
            .iter()
            .map(|context| context.name.as_str())
            .collect();
        write!(f, "{}", names.join("::"))
    }
}
