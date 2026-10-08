/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Private declaration layer: what a document declared, before resolution.

use crate::diagnostic::SourceLocation;
use crate::node::{Node, NodeObject};
use crate::shape::ShapeType;
use crate::shape_id::ShapeId;
use indexmap::IndexMap;

/// Applied traits, keyed by absolute trait shape ID, in source order.
pub(crate) type TraitMap = IndexMap<ShapeId, Node>;

/// Named members keyed by member name, in declaration order.
pub(crate) type Members = IndexMap<String, MemberDecl>;

/// A parsed document.
#[derive(Debug, Clone, Default)]
pub(crate) struct Document {
    pub(crate) metadata: NodeObject,
    pub(crate) shapes: IndexMap<ShapeId, ShapeDecl>,
}

#[derive(Debug, Clone)]
pub(crate) struct ShapeDecl {
    pub(crate) id: ShapeId,
    pub(crate) kind: DeclKind,
    pub(crate) traits: TraitMap,
    pub(crate) source: SourceLocation,
}

impl ShapeDecl {
    pub(crate) fn shape_type(&self) -> ShapeType {
        self.kind.shape_type()
    }

    /// Members in declaration order: list `member`, map `key` then `value`, or named members.
    pub(crate) fn members(&self) -> MemberIter<'_> {
        match &self.kind {
            DeclKind::List { member } => MemberIter::Fixed([Some(member), None].into_iter()),
            DeclKind::Map { key, value } => MemberIter::Fixed([Some(key), Some(value)].into_iter()),
            DeclKind::Structure(m)
            | DeclKind::Union(m)
            | DeclKind::Enum(m)
            | DeclKind::IntEnum(m) => MemberIter::Named(m.values()),
            _ => MemberIter::Fixed([None, None].into_iter()),
        }
    }

    pub(crate) fn member(&self, name: &str) -> Option<&MemberDecl> {
        match &self.kind {
            DeclKind::List { member } => (name == "member").then_some(member),
            DeclKind::Map { key, value } => match name {
                "key" => Some(key),
                "value" => Some(value),
                _ => None,
            },
            DeclKind::Structure(m)
            | DeclKind::Union(m)
            | DeclKind::Enum(m)
            | DeclKind::IntEnum(m) => m.get(name),
            _ => None,
        }
    }

    pub(crate) fn member_count(&self) -> usize {
        match &self.kind {
            DeclKind::List { .. } => 1,
            DeclKind::Map { .. } => 2,
            DeclKind::Structure(m)
            | DeclKind::Union(m)
            | DeclKind::Enum(m)
            | DeclKind::IntEnum(m) => m.len(),
            _ => 0,
        }
    }
}

#[derive(Clone)]
pub(crate) enum MemberIter<'a> {
    Fixed(std::array::IntoIter<Option<&'a MemberDecl>, 2>),
    Named(indexmap::map::Values<'a, String, MemberDecl>),
}

impl<'a> Iterator for MemberIter<'a> {
    type Item = &'a MemberDecl;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            MemberIter::Fixed(it) => it.by_ref().flatten().next(),
            MemberIter::Named(it) => it.next(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MemberDecl {
    /// `Container$name`
    pub(crate) id: ShapeId,
    pub(crate) target: ShapeId,
    pub(crate) traits: TraitMap,
    pub(crate) source: SourceLocation,
}

impl MemberDecl {
    pub(crate) fn name(&self) -> &str {
        self.id
            .member()
            .expect("member IDs always have a member component")
    }
}

#[derive(Debug, Clone)]
pub(crate) enum DeclKind {
    /// Blob, boolean, document, string, numbers, and timestamp.
    Simple(ShapeType),
    List {
        member: MemberDecl,
    },
    Map {
        key: MemberDecl,
        value: MemberDecl,
    },
    Structure(Members),
    Union(Members),
    Enum(Members),
    IntEnum(Members),
    Service(ServiceDecl),
    Operation(OperationDecl),
    Resource(ResourceDecl),
}

impl DeclKind {
    pub(crate) fn shape_type(&self) -> ShapeType {
        match self {
            DeclKind::Simple(t) => *t,
            DeclKind::List { .. } => ShapeType::List,
            DeclKind::Map { .. } => ShapeType::Map,
            DeclKind::Structure(_) => ShapeType::Structure,
            DeclKind::Union(_) => ShapeType::Union,
            DeclKind::Enum(_) => ShapeType::Enum,
            DeclKind::IntEnum(_) => ShapeType::IntEnum,
            DeclKind::Service(_) => ShapeType::Service,
            DeclKind::Operation(_) => ShapeType::Operation,
            DeclKind::Resource(_) => ShapeType::Resource,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ServiceDecl {
    pub(crate) version: Option<String>,
    pub(crate) operations: Vec<ShapeId>,
    pub(crate) resources: Vec<ShapeId>,
    pub(crate) errors: Vec<ShapeId>,
    pub(crate) rename: IndexMap<ShapeId, String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OperationDecl {
    /// `None` when omitted; the resolved view normalizes to `smithy.api#Unit`.
    pub(crate) input: Option<ShapeId>,
    pub(crate) output: Option<ShapeId>,
    pub(crate) errors: Vec<ShapeId>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ResourceDecl {
    pub(crate) identifiers: IndexMap<String, ShapeId>,
    pub(crate) properties: IndexMap<String, ShapeId>,
    pub(crate) create: Option<ShapeId>,
    pub(crate) put: Option<ShapeId>,
    pub(crate) read: Option<ShapeId>,
    pub(crate) update: Option<ShapeId>,
    pub(crate) delete: Option<ShapeId>,
    pub(crate) list: Option<ShapeId>,
    pub(crate) operations: Vec<ShapeId>,
    pub(crate) collection_operations: Vec<ShapeId>,
    pub(crate) resources: Vec<ShapeId>,
}

impl ResourceDecl {
    /// Lifecycle operations as `(property name, target)` in JSON AST order.
    pub(crate) fn lifecycle(&self) -> [(&'static str, Option<&ShapeId>); 6] {
        [
            ("create", self.create.as_ref()),
            ("put", self.put.as_ref()),
            ("read", self.read.as_ref()),
            ("update", self.update.as_ref()),
            ("delete", self.delete.as_ref()),
            ("list", self.list.as_ref()),
        ]
    }
}
