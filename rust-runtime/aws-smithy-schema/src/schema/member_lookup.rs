/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Resolving wire field names to structure members during deserialization.

use crate::extension::SchemaExtensionKey;
use crate::Schema;

/// Structures with at least this many members resolve out-of-order fields through a
/// cached index rather than a scan.
///
/// Measured on the JSON and CBOR deserializers, an index lookup (which includes the
/// cost of the schema extension lookup) is slower than a scan up to 6 members and
/// faster from 20. Member counts across AWS models are p50=2, p90=6 and p99=20, so
/// only the widest structures build an index.
pub const WIDE_STRUCT_MEMBERS: usize = 16;

/// Which name a member has on the wire.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireName {
    /// The Smithy member name.
    MemberName,
    /// The `@jsonName` value when present, otherwise the member name.
    JsonName,
    /// The `@xmlName` value when present, otherwise the member name.
    XmlName,
}

impl WireName {
    #[inline]
    fn of<'s>(self, member: &'s Schema<'_>) -> Option<&'s str> {
        match self {
            WireName::MemberName => member.member_name(),
            WireName::JsonName => match member.json_name() {
                Some(name) => Some(name.value()),
                None => member.member_name(),
            },
            WireName::XmlName => match member.xml_name() {
                Some(name) => Some(name.value()),
                None => member.member_name(),
            },
        }
    }

    #[inline]
    fn matches(self, member: &Schema<'_>, name: &str) -> bool {
        self.of(member) == Some(name)
    }

    fn index_key(self) -> &'static SchemaExtensionKey<MemberIndex> {
        match self {
            WireName::MemberName => &MEMBER_NAME_INDEX,
            WireName::JsonName => &JSON_NAME_INDEX,
            WireName::XmlName => &XML_NAME_INDEX,
        }
    }
}

/// Member positions grouped by wire-name length.
///
/// Names in one structure mostly differ in length, so a lookup usually compares
/// against one or two candidates. Stores positions only; the names stay on the member
/// schemas.
#[derive(Debug)]
pub(crate) struct MemberIndex {
    /// `positions[starts[len]..starts[len + 1]]` are the members whose name is `len`
    /// bytes, in member order.
    starts: Box<[u32]>,
    positions: Box<[u32]>,
}

impl MemberIndex {
    fn new(schema: &Schema<'_>, wire_name: WireName) -> Self {
        let members = schema.members();
        let max_len = members
            .iter()
            .filter_map(|member| wire_name.of(member))
            .map(str::len)
            .max();
        let Some(max_len) = max_len else {
            return Self {
                starts: Box::new([]),
                positions: Box::new([]),
            };
        };
        // Counting sort by length.
        let mut starts = vec![0u32; max_len + 2];
        for len in members.iter().filter_map(|m| wire_name.of(m)).map(str::len) {
            starts[len + 1] += 1;
        }
        for i in 1..starts.len() {
            starts[i] += starts[i - 1];
        }
        let mut fill = starts.clone();
        let mut positions = vec![0u32; starts[max_len + 1] as usize];
        for (position, member) in members.iter().enumerate() {
            if let Some(name) = wire_name.of(member) {
                positions[fill[name.len()] as usize] = position as u32;
                fill[name.len()] += 1;
            }
        }
        Self {
            starts: starts.into_boxed_slice(),
            positions: positions.into_boxed_slice(),
        }
    }

    #[inline]
    fn find(&self, members: &[&Schema<'_>], wire_name: WireName, name: &str) -> Option<usize> {
        let start = *self.starts.get(name.len())? as usize;
        let end = *self.starts.get(name.len() + 1)? as usize;
        self.positions[start..end]
            .iter()
            .map(|&position| position as usize)
            .find(|&position| wire_name.matches(members[position], name))
    }
}

static MEMBER_NAME_INDEX: SchemaExtensionKey<MemberIndex> =
    SchemaExtensionKey::new(|schema| MemberIndex::new(schema, WireName::MemberName));
static JSON_NAME_INDEX: SchemaExtensionKey<MemberIndex> =
    SchemaExtensionKey::new(|schema| MemberIndex::new(schema, WireName::JsonName));
static XML_NAME_INDEX: SchemaExtensionKey<MemberIndex> =
    SchemaExtensionKey::new(|schema| MemberIndex::new(schema, WireName::XmlName));

/// How many members past the last match a wide structure scans before using its
/// index. Covers the common case of a few omitted optional members.
const WIDE_FORWARD_PROBES: usize = 4;

/// Resolves the fields of one structure to its members, in any order.
///
/// Create one cursor for each structure being read and pass it every field name of
/// that structure.
///
/// A deserializer reads a structure's fields by name and has to find the member
/// schema for each one. Scanning every member for every field makes a structure
/// `O(M²)` in its member count. A cursor removes most of that cost:
///
/// - Serializers, including every smithy-rs serializer and the services AWS SDKs
///   talk to, emit fields in model order. The cursor remembers where the last field
///   matched and checks the following members first, so an in-order structure costs
///   one comparison per field plus one per omitted member.
/// - A field out of order wraps around the members once, so it costs no more
///   comparisons than the full scan it replaces.
/// - Structures with at least [`WIDE_STRUCT_MEMBERS`] members look out-of-order
///   fields up in a per-structure index instead, cached on the structure schema as a
///   [schema extension](crate::extension) and held by the cursor after its first use.
///   Below that size, the scan is cheaper than an extension lookup.
///
/// ```
/// use aws_smithy_schema::member_lookup::{MemberCursor, WireName};
/// use aws_smithy_schema::{shape_id, Schema, ShapeType};
///
/// static A: Schema<'static> = Schema::new_member(shape_id!("ns", "S", "a"), ShapeType::String, "a", 0);
/// static B: Schema<'static> = Schema::new_member(shape_id!("ns", "S", "b"), ShapeType::String, "b", 1);
/// static S: Schema<'static> = Schema::new_struct(shape_id!("ns", "S"), ShapeType::Structure, &[&A, &B]);
///
/// // One cursor per structure being read.
/// let mut cursor = MemberCursor::new(&S, WireName::MemberName);
/// assert_eq!(cursor.resolve("a").unwrap().member_index(), Some(0));
/// assert_eq!(cursor.resolve("b").unwrap().member_index(), Some(1));
/// assert!(cursor.resolve("unknown").is_none());
/// ```
#[derive(Debug, Clone)]
pub struct MemberCursor<'s> {
    schema: &'s Schema<'s>,
    members: &'s [&'s Schema<'s>],
    wire_name: WireName,
    /// Position of the member expected next.
    next: usize,
    /// The structure's index, once a wide structure has needed it.
    index: Option<&'s MemberIndex>,
}

impl<'s> MemberCursor<'s> {
    /// Creates a cursor over the members of `schema`, matched by `wire_name`.
    ///
    /// A schema that is not a structure or union has no members, so every field is
    /// unknown.
    #[inline]
    pub fn new(schema: &'s Schema<'s>, wire_name: WireName) -> Self {
        Self {
            schema,
            members: schema.members(),
            wire_name,
            next: 0,
            index: None,
        }
    }

    /// Returns the member whose wire name is `name`, or `None` if there is none.
    ///
    /// Smithy requires wire names to be unique within a structure. If several members
    /// share one anyway, which of them is returned is unspecified.
    #[inline]
    pub fn resolve(&mut self, name: &str) -> Option<&'s Schema<'s>> {
        let position = self.position(name)?;
        self.next = position + 1;
        Some(self.members[position])
    }

    #[inline]
    fn position(&mut self, name: &str) -> Option<usize> {
        let (members, wire_name) = (self.members, self.wire_name);
        let len = members.len();
        let start = if self.next < len { self.next } else { 0 };
        // The member after the last match: the hit for every in-order field.
        if wire_name.matches(members.get(start)?, name) {
            return Some(start);
        }
        if len < WIDE_STRUCT_MEMBERS {
            // Forward from the expected member, then wrap: at most one full pass.
            for (i, member) in members.iter().enumerate().skip(start + 1) {
                if wire_name.matches(member, name) {
                    return Some(i);
                }
            }
            for (i, member) in members[..start].iter().enumerate() {
                if wire_name.matches(member, name) {
                    return Some(i);
                }
            }
            return None;
        }
        self.position_wide(start, name)
    }

    #[inline(never)]
    fn position_wide(&mut self, start: usize, name: &str) -> Option<usize> {
        let (members, wire_name) = (self.members, self.wire_name);
        let probe_end = (start + WIDE_FORWARD_PROBES).min(members.len());
        for (i, member) in members[..probe_end].iter().enumerate().skip(start + 1) {
            if wire_name.matches(member, name) {
                return Some(i);
            }
        }
        let schema = self.schema;
        self.index
            .get_or_insert_with(|| schema.extension(wire_name.index_key()))
            .find(members, wire_name, name)
    }
}

/// Finds the member of `schema` whose wire name is `name`.
///
/// Uses the cached index for wide structures. Prefer a [`MemberCursor`] when
/// resolving several fields of one structure.
pub(crate) fn find_member<'s>(
    schema: &'s Schema<'s>,
    wire_name: WireName,
    name: &str,
) -> Option<&'s Schema<'s>> {
    MemberCursor::new(schema, wire_name).resolve(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{shape_id, ShapeType};

    // Names of distinct lengths and collisions, so both the index buckets and the
    // within-bucket comparisons are exercised.
    const NAMES: [&str; 24] = [
        "a", "bb", "cc", "ddd", "eeee", "f", "gggggg", "hh", "iii", "jjjj", "kkkkk", "ll",
        "mmmmmmmm", "n", "oo", "ppp", "qqqq", "rrrrr", "ssssss", "ttttttt", "u", "vv", "www",
        "xxxx",
    ];

    fn members(count: usize) -> Vec<&'static Schema<'static>> {
        (0..count)
            .map(|i| {
                let schema =
                    Schema::new_member(shape_id!("ns", "S", "m"), ShapeType::String, NAMES[i], i);
                // Every third member is renamed, to exercise the wire-name variants.
                let schema = if i % 3 == 0 {
                    schema
                        .with_json_name(Box::leak(format!("json_{}", NAMES[i]).into_boxed_str()))
                        .with_xml_name(Box::leak(format!("xml_{}", NAMES[i]).into_boxed_str()))
                } else {
                    schema
                };
                &*Box::leak(Box::new(schema))
            })
            .collect()
    }

    fn structure(count: usize) -> &'static Schema<'static> {
        let members: &'static [&'static Schema<'static>] = members(count).leak();
        Box::leak(Box::new(Schema::new_struct(
            shape_id!("ns", "S"),
            ShapeType::Structure,
            members,
        )))
    }

    /// The reference behavior. Wire names in the test structures are unique.
    fn scan<'s>(schema: &'s Schema<'s>, wire_name: WireName, name: &str) -> Option<usize> {
        schema
            .members()
            .iter()
            .position(|m| wire_name.matches(m, name))
    }

    fn wire_names(schema: &Schema<'_>, wire_name: WireName) -> Vec<String> {
        schema
            .members()
            .iter()
            .map(|m| wire_name.of(m).unwrap().to_owned())
            .collect()
    }

    const WIRE_NAMES: [WireName; 3] = [WireName::MemberName, WireName::JsonName, WireName::XmlName];

    fn check(schema: &'static Schema<'static>, wire_name: WireName, order: &[String]) {
        let mut cursor = MemberCursor::new(schema, wire_name);
        for name in order {
            let got = cursor.resolve(name).and_then(|m| m.member_index());
            assert_eq!(got, scan(schema, wire_name, name), "{wire_name:?} {name}");
        }
    }

    #[test]
    fn matches_scan_in_every_order_and_size() {
        // Sizes on both sides of the index threshold.
        for count in [0, 1, 2, 6, WIDE_STRUCT_MEMBERS - 1, WIDE_STRUCT_MEMBERS, 24] {
            let schema = structure(count);
            for wire_name in WIRE_NAMES {
                let names = wire_names(schema, wire_name);
                let mut extra = vec!["unknown".to_owned(), String::new(), "__type".to_owned()];
                // Member names that are not the wire name must not match.
                extra.extend(
                    schema
                        .members()
                        .iter()
                        .filter_map(|m| m.member_name().map(str::to_owned)),
                );

                check(schema, wire_name, &names);
                let reversed: Vec<_> = names.iter().rev().cloned().collect();
                check(schema, wire_name, &reversed);
                // Sparse: every third field, as when optional members are omitted.
                let sparse: Vec<_> = names.iter().step_by(3).cloned().collect();
                check(schema, wire_name, &sparse);
                // Repeated and interleaved with unknown fields.
                let mut mixed = Vec::new();
                for name in names.iter().chain(names.iter()) {
                    mixed.push(name.clone());
                    mixed.extend(extra.iter().take(2).cloned());
                }
                check(schema, wire_name, &mixed);
                check(schema, wire_name, &extra);
            }
        }
    }

    #[test]
    fn wide_structure_caches_its_index() {
        let schema = structure(24);
        let mut cursor = MemberCursor::new(schema, WireName::MemberName);
        // In order: each match moves the cursor past it, so no index is needed.
        for (position, name) in NAMES.iter().enumerate() {
            assert_eq!(cursor.resolve(name).unwrap().member_index(), Some(position));
            assert_eq!(cursor.next, position + 1);
        }
        assert!(cursor.index.is_none());
        // Out of order (the cursor wrapped to the first member): resolved through the
        // index, which is cached on the schema.
        assert_eq!(cursor.resolve(NAMES[10]).unwrap().member_index(), Some(10));
        let held = cursor.index.expect("index used") as *const MemberIndex;
        assert!(std::ptr::eq(held, schema.extension(&MEMBER_NAME_INDEX)));
        assert_eq!(schema.extension(&MEMBER_NAME_INDEX).positions.len(), 24);
    }

    #[test]
    fn narrow_structure_never_builds_an_index() {
        let schema = structure(WIDE_STRUCT_MEMBERS - 1);
        let mut cursor = MemberCursor::new(schema, WireName::MemberName);
        for name in NAMES[..WIDE_STRUCT_MEMBERS - 1].iter().rev() {
            assert!(cursor.resolve(name).is_some());
        }
        assert!(cursor.index.is_none());
    }

    #[test]
    fn non_structures_have_no_members() {
        let mut cursor = MemberCursor::new(&crate::prelude::STRING, WireName::MemberName);
        assert!(cursor.resolve("a").is_none());
    }
}
