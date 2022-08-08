/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use rustdoc_types::{Item, Span};
use std::fmt;

/// Component type for components in a [`Path`].
#[derive(Copy, Clone, Debug)]
pub enum ComponentType {
    AssocConst,
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

/// Represents one component in a [`Path`].
#[derive(Clone, Debug)]
struct Component {
    typ: ComponentType,
    name: String,
    span: Option<Span>,
}

impl Component {
    fn new(typ: ComponentType, name: String, span: Option<Span>) -> Self {
        Self { typ, name, span }
    }
}

/// Represents the full path to an item being visited by [`Visitor`](crate::visitor::Visitor).
///
/// This is equivalent to the type path of that item, which has to be re-assembled since
/// it is lost in the flat structure of the Rustdoc JSON output.
#[derive(Clone, Debug)]
pub struct Path {
    stack: Vec<Component>,
}

impl Path {
    pub fn new(crate_name: &str) -> Self {
        Self {
            stack: vec![Component::new(
                ComponentType::Crate,
                crate_name.into(),
                None,
            )],
        }
    }

    pub fn push(&mut self, typ: ComponentType, item: &Item) {
        self.push_raw(typ, item.name.as_ref().expect("name"), item.span.as_ref());
    }

    pub fn push_raw(&mut self, typ: ComponentType, name: &str, span: Option<&Span>) {
        self.stack
            .push(Component::new(typ, name.into(), span.cloned()));
    }

    /// Returns the span (file + beginning and end positions) of the last [`Component`] in the stack.
    pub fn last_span(&self) -> Option<&Span> {
        self.stack.last().and_then(|c| c.span.as_ref())
    }

    /// Returns the [`ComponentType`] of the last [`Component`] in the path.
    pub fn last_typ(&self) -> Option<ComponentType> {
        self.stack.last().map(|c| c.typ)
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self
            .stack
            .iter()
            .map(|component| component.name.as_str())
            .collect();
        write!(f, "{}", names.join("::"))
    }
}
