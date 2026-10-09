/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! `StructuralV1` validation.
//!
//! The loader has already enforced the JSON AST grammar, supported version and features,
//! absolute IDs, identifiers, and the prelude namespace (rules 1 and 2 in part). This module
//! enforces the rest:
//!
//! 1. Shape IDs are case-insensitively unique across the document and prelude.
//! 3. Every member target and kind-specific reference resolves to a root shape.
//! 4. Members target data shapes, never services, resources, operations, members, or trait
//!    definitions.
//! 5. An applied trait that resolves to a shape resolves to a trait definition. Missing
//!    definitions are errors only with `require_trait_definitions`.
//! 6. Map keys target `string` or `enum`. (Lists and maps having their members is
//!    guaranteed by the loader.)
//! 7. Structure members are case-insensitively unique.
//! 8. Unions are non-empty with case-insensitively unique members.
//! 9. Enums and intEnums are non-empty, members target `smithy.api#Unit`, and effective
//!    values are valid and unique.
//! 10. Service operations, resources, errors, and renames are valid.
//! 11. Operation input/output target structures and errors target `@error` structures.
//! 12. Resource identifiers, properties, lifecycle operations, operations, and children are
//!     valid, and resource containment is acyclic.
//! 13. `smithy.api#Unit` is only targeted by operation input/output and union, enum, and
//!     intEnum members.
//! 14. Member IDs belong to their containers. This holds by construction: the loader derives
//!     every member ID from its container.
//!
//! Only document shapes are validated. The embedded prelude is trusted, and a unit test
//! checks that it passes these rules.

use crate::ast::{DeclKind, Document, MemberDecl, Members, ShapeDecl, TraitMap};
use crate::diagnostic::{
    escape_pointer_token, Diagnostic, DiagnosticCode, DiagnosticSet, SourceLocation,
};
use crate::loader::ModelLoader;
use crate::model::unit_id;
use crate::node::Node;
use crate::shape::ShapeType;
use crate::shape_id::ShapeId;
use crate::traits;
use std::collections::HashMap;

/// Validates `document` against `StructuralV1`, resolving references against `prelude`.
pub(crate) fn validate(
    document: &Document,
    prelude: Option<&Document>,
    options: &ModelLoader,
) -> DiagnosticSet {
    let mut validator = Validator {
        document,
        prelude,
        require_trait_definitions: options.require_trait_definitions,
        diagnostics: DiagnosticSet::default(),
    };
    validator.shape_id_case_conflicts();
    for shape in document.shapes.values() {
        validator.shape(shape);
    }
    validator.resource_cycles();
    validator.diagnostics
}

struct Validator<'a> {
    document: &'a Document,
    prelude: Option<&'a Document>,
    require_trait_definitions: bool,
    diagnostics: DiagnosticSet,
}

/// Members, operation input/output, and resource properties may only target data shapes.
fn is_data_shape(shape_type: ShapeType) -> bool {
    !shape_type.is_service_shape() && shape_type != ShapeType::Member
}

fn describe(allowed: &[ShapeType]) -> String {
    let names: Vec<_> = allowed.iter().map(|t| t.as_str()).collect();
    names.join(" or ")
}

impl<'a> Validator<'a> {
    fn error(&mut self, code: DiagnosticCode, message: String, location: SourceLocation) {
        self.diagnostics
            .push(Diagnostic::error(code, message, Some(location)));
    }

    fn conflict(&mut self, message: String, location: &SourceLocation, first: &SourceLocation) {
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::CaseConflict,
                message,
                Some(location.clone()),
            )
            .with_related(first.clone()),
        );
    }

    fn resolve(&self, id: &str) -> Option<&'a ShapeDecl> {
        self.document
            .shapes
            .get(id)
            .or_else(|| self.prelude?.shapes.get(id))
    }

    /// Resolves an ordinary shape reference to a root shape.
    fn reference(&mut self, id: &ShapeId, location: &SourceLocation) -> Option<&'a ShapeDecl> {
        if id.is_member() {
            self.error(
                DiagnosticCode::InvalidTarget,
                format!("`{id}` is a member; references must target root shapes"),
                location.clone(),
            );
            return None;
        }
        let target = self.resolve(id.as_str());
        if target.is_none() {
            self.error(
                DiagnosticCode::UnresolvedReference,
                format!("`{id}` does not resolve to a shape"),
                location.clone(),
            );
        }
        target
    }

    /// Resolves a reference that must target one of `allowed`.
    fn expect(
        &mut self,
        id: &ShapeId,
        location: SourceLocation,
        allowed: &[ShapeType],
        what: &str,
    ) -> Option<&'a ShapeDecl> {
        let target = self.reference(id, &location)?;
        let actual = target.shape_type();
        if allowed.contains(&actual) {
            Some(target)
        } else {
            self.error(
                DiagnosticCode::InvalidTarget,
                format!(
                    "{what} must target a {}, but `{id}` is a {actual}",
                    describe(allowed)
                ),
                location,
            );
            None
        }
    }

    fn error_reference(&mut self, id: &ShapeId, location: SourceLocation, what: &str) {
        let Some(target) = self.expect(id, location.clone(), &[ShapeType::Structure], what) else {
            return;
        };
        if !target.traits.contains_key(traits::ERROR) {
            self.error(
                DiagnosticCode::InvalidTarget,
                format!("{what} must target an error structure, but `{id}` has no `@error` trait"),
                location,
            );
        }
    }

    fn reference_list(
        &mut self,
        shape: &ShapeDecl,
        property: &str,
        ids: &[ShapeId],
        allowed: ShapeType,
    ) {
        for (index, id) in ids.iter().enumerate() {
            let location = shape.source.child(&format!("/{property}/{index}/target"));
            self.expect(id, location, &[allowed], &format!("`{property}`"));
        }
    }

    fn applied_traits(&mut self, applied: &TraitMap, owner: &SourceLocation) {
        for id in applied.keys() {
            let location = owner.child(&format!("/traits/{}", escape_pointer_token(id.as_str())));
            match self.resolve(id.as_str()) {
                Some(definition) if !definition.traits.contains_key(traits::TRAIT) => self.error(
                    DiagnosticCode::InvalidTrait,
                    format!("`{id}` is applied as a trait but is not a trait definition"),
                    location,
                ),
                Some(_) => {}
                None if self.require_trait_definitions => self.error(
                    DiagnosticCode::InvalidTrait,
                    format!("trait `{id}` has no definition"),
                    location,
                ),
                None => {}
            }
        }
    }

    fn shape_id_case_conflicts(&mut self) {
        let mut seen: HashMap<String, &'a ShapeDecl> = HashMap::new();
        let document = self.document;
        // Prelude first, so a conflict is reported on the user's declaration.
        let prelude = self.prelude.into_iter().flat_map(|p| p.shapes.values());
        for decl in prelude.chain(document.shapes.values()) {
            let key = decl.id.as_str().to_ascii_lowercase();
            match seen.get(&key) {
                Some(first) => self.conflict(
                    format!(
                        "`{}` conflicts case-insensitively with `{}`",
                        decl.id, first.id
                    ),
                    &decl.source,
                    &first.source,
                ),
                None => {
                    seen.insert(key, decl);
                }
            }
        }
    }

    fn member_case_conflicts(&mut self, container: &ShapeDecl, members: &'a Members) {
        let mut seen: HashMap<String, &'a MemberDecl> = HashMap::new();
        for member in members.values() {
            let key = member.name().to_ascii_lowercase();
            match seen.get(&key) {
                Some(first) => self.conflict(
                    format!(
                        "member `{}` of `{}` conflicts case-insensitively with `{}`",
                        member.name(),
                        container.id,
                        first.name()
                    ),
                    &member.source,
                    &first.source,
                ),
                None => {
                    seen.insert(key, member);
                }
            }
        }
    }

    fn shape(&mut self, shape: &'a ShapeDecl) {
        self.applied_traits(&shape.traits, &shape.source);
        for member in shape.members() {
            self.member(shape, member);
        }
        match &shape.kind {
            DeclKind::Simple(_) | DeclKind::List { .. } | DeclKind::Map { .. } => {}
            DeclKind::Structure(members) => self.member_case_conflicts(shape, members),
            DeclKind::Union(members) => {
                self.require_members(shape, members, "union");
                self.member_case_conflicts(shape, members);
            }
            DeclKind::Enum(members) => {
                self.require_members(shape, members, "enum");
                self.member_case_conflicts(shape, members);
                self.enum_values(shape, members, false);
            }
            DeclKind::IntEnum(members) => {
                self.require_members(shape, members, "intEnum");
                self.member_case_conflicts(shape, members);
                self.enum_values(shape, members, true);
            }
            DeclKind::Service(_) => self.service(shape),
            DeclKind::Operation(_) => self.operation(shape),
            DeclKind::Resource(_) => self.resource(shape),
        }
    }

    fn member(&mut self, container: &ShapeDecl, member: &MemberDecl) {
        self.applied_traits(&member.traits, &member.source);
        let location = member.source.child("/target");
        let Some(target) = self.reference(&member.target, &location) else {
            return;
        };
        let container_type = container.shape_type();
        let is_enum = matches!(container_type, ShapeType::Enum | ShapeType::IntEnum);
        if member.target == *unit_id() {
            if !matches!(container_type, ShapeType::Union) && !is_enum {
                self.error(
                    DiagnosticCode::InvalidTarget,
                    format!(
                        "`{}` cannot target `smithy.api#Unit`; only union, enum, and intEnum \
                         members and operation input/output can",
                        member.id
                    ),
                    location,
                );
            }
            return;
        }
        if is_enum {
            self.error(
                DiagnosticCode::InvalidTarget,
                format!(
                    "{container_type} member `{}` must target `smithy.api#Unit`",
                    member.id
                ),
                location,
            );
            return;
        }
        let target_type = target.shape_type();
        if !is_data_shape(target_type) {
            self.error(
                DiagnosticCode::InvalidTarget,
                format!(
                    "member `{}` targets {target_type} `{}`; members must target data shapes",
                    member.id, member.target
                ),
                location,
            );
        } else if target.traits.contains_key(traits::TRAIT) {
            self.error(
                DiagnosticCode::InvalidTarget,
                format!(
                    "member `{}` targets trait definition `{}`; members cannot target traits",
                    member.id, member.target
                ),
                location,
            );
        } else if container_type == ShapeType::Map
            && member.name() == "key"
            && !matches!(target_type, ShapeType::String | ShapeType::Enum)
        {
            self.error(
                DiagnosticCode::InvalidTarget,
                format!(
                    "map key `{}` must target a string or enum, but `{}` is a {target_type}",
                    member.id, member.target
                ),
                location,
            );
        }
    }

    fn require_members(&mut self, shape: &ShapeDecl, members: &Members, kind: &str) {
        if members.is_empty() {
            self.error(
                DiagnosticCode::InvalidShape,
                format!("{kind} `{}` must have at least one member", shape.id),
                shape.source.clone(),
            );
        }
    }

    fn enum_values(&mut self, shape: &ShapeDecl, members: &'a Members, int_enum: bool) {
        let mut seen: HashMap<String, &'a MemberDecl> = HashMap::new();
        for member in members.values() {
            let location = member
                .source
                .child(&format!("/traits/{}", traits::ENUM_VALUE));
            let value = match (int_enum, member.traits.get(traits::ENUM_VALUE)) {
                (false, None) => Some(member.name().to_owned()),
                (false, Some(Node::String(value))) if !value.is_empty() => Some(value.clone()),
                (false, Some(_)) => {
                    self.error(
                        DiagnosticCode::InvalidTrait,
                        format!(
                            "enum member `{}` must have a non-empty string `@enumValue`",
                            member.id
                        ),
                        location,
                    );
                    None
                }
                (true, None) => {
                    self.error(
                        DiagnosticCode::InvalidShape,
                        format!(
                            "intEnum member `{}` requires an integer `@enumValue`",
                            member.id
                        ),
                        member.source.clone(),
                    );
                    None
                }
                (true, Some(node)) => {
                    let value = node
                        .as_number()
                        .and_then(|n| n.as_i64())
                        .and_then(|n| i32::try_from(n).ok());
                    if value.is_none() {
                        self.error(
                            DiagnosticCode::InvalidTrait,
                            format!(
                                "intEnum member `{}` must have a 32-bit integer `@enumValue`",
                                member.id
                            ),
                            location,
                        );
                    }
                    value.map(|v| v.to_string())
                }
            };
            let Some(value) = value else { continue };
            match seen.get(&value) {
                Some(first) => {
                    let diagnostic = Diagnostic::error(
                        DiagnosticCode::InvalidShape,
                        format!(
                            "`{}` members `{}` and `{}` have the same value `{value}`",
                            shape.id,
                            first.name(),
                            member.name()
                        ),
                        Some(member.source.clone()),
                    )
                    .with_related(first.source.clone());
                    self.diagnostics.push(diagnostic);
                }
                None => {
                    seen.insert(value, member);
                }
            }
        }
    }

    fn service(&mut self, shape: &ShapeDecl) {
        let DeclKind::Service(service) = &shape.kind else {
            return;
        };
        self.reference_list(
            shape,
            "operations",
            &service.operations,
            ShapeType::Operation,
        );
        self.reference_list(shape, "resources", &service.resources, ShapeType::Resource);
        for (index, id) in service.errors.iter().enumerate() {
            let location = shape.source.child(&format!("/errors/{index}/target"));
            self.error_reference(id, location, "service `errors`");
        }
        let mut names: HashMap<String, (&ShapeId, SourceLocation)> = HashMap::new();
        for (id, name) in &service.rename {
            let location = shape
                .source
                .child(&format!("/rename/{}", escape_pointer_token(id.as_str())));
            self.reference(id, &location);
            if !ShapeId::is_valid_identifier(name) {
                self.error(
                    DiagnosticCode::InvalidProperty,
                    format!("rename of `{id}` to `{name}` is not a valid identifier"),
                    location,
                );
                continue;
            }
            match names.get(&name.to_ascii_lowercase()) {
                Some((first, first_location)) => {
                    let first_location = first_location.clone();
                    self.conflict(
                        format!(
                            "`{id}` and `{first}` are renamed to case-insensitively equal names"
                        ),
                        &location,
                        &first_location,
                    );
                }
                None => {
                    names.insert(name.to_ascii_lowercase(), (id, location));
                }
            }
        }
    }

    fn operation(&mut self, shape: &ShapeDecl) {
        let DeclKind::Operation(operation) = &shape.kind else {
            return;
        };
        for (property, id) in [("input", &operation.input), ("output", &operation.output)] {
            // An omitted input/output is normalized to `smithy.api#Unit`, so it must resolve
            // too (it does not when the prelude is disabled and the document lacks it).
            let (id, location) = match id {
                Some(id) => (id, shape.source.child(&format!("/{property}/target"))),
                None => (unit_id(), shape.source.clone()),
            };
            self.expect(
                id,
                location,
                &[ShapeType::Structure],
                &format!("operation `{property}`"),
            );
        }
        for (index, id) in operation.errors.iter().enumerate() {
            let location = shape.source.child(&format!("/errors/{index}/target"));
            self.error_reference(id, location, "operation `errors`");
        }
    }

    fn resource(&mut self, shape: &ShapeDecl) {
        let DeclKind::Resource(resource) = &shape.kind else {
            return;
        };
        for (name, id) in &resource.identifiers {
            let location = shape.source.child(&format!(
                "/identifiers/{}/target",
                escape_pointer_token(name)
            ));
            self.expect(
                id,
                location,
                &[ShapeType::String, ShapeType::Enum],
                "resource identifier",
            );
        }
        for (name, id) in &resource.properties {
            let location = shape.source.child(&format!(
                "/properties/{}/target",
                escape_pointer_token(name)
            ));
            let Some(target) = self.reference(id, &location) else {
                continue;
            };
            if !is_data_shape(target.shape_type()) || id == unit_id() {
                self.error(
                    DiagnosticCode::InvalidTarget,
                    format!(
                        "resource property `{name}` must target a data shape, but `{id}` is a {}",
                        target.shape_type()
                    ),
                    location,
                );
            }
        }
        for (property, id) in resource.lifecycle() {
            if let Some(id) = id {
                let location = shape.source.child(&format!("/{property}/target"));
                self.expect(
                    id,
                    location,
                    &[ShapeType::Operation],
                    &format!("resource `{property}`"),
                );
            }
        }
        self.reference_list(
            shape,
            "operations",
            &resource.operations,
            ShapeType::Operation,
        );
        self.reference_list(
            shape,
            "collectionOperations",
            &resource.collection_operations,
            ShapeType::Operation,
        );
        self.reference_list(shape, "resources", &resource.resources, ShapeType::Resource);
    }

    /// Reports each resource-containment cycle using an iterative depth-first search.
    fn resource_cycles(&mut self) {
        let document = self.document;
        let resource = |id: &str| match document.shapes.get(id).map(|d| &d.kind) {
            Some(DeclKind::Resource(r)) => Some(r),
            _ => None,
        };
        // `false` while on the current path, `true` once fully explored.
        let mut done: HashMap<&'a ShapeId, bool> = HashMap::new();
        for decl in document.shapes.values() {
            if resource(decl.id.as_str()).is_none() || done.contains_key(&decl.id) {
                continue;
            }
            let mut stack: Vec<(&'a ShapeId, usize)> = vec![(&decl.id, 0)];
            done.insert(&decl.id, false);
            while let Some(&(id, index)) = stack.last() {
                let children = resource(id.as_str()).map_or(&[][..], |r| &r.resources);
                let Some(child) = children.get(index) else {
                    done.insert(id, true);
                    stack.pop();
                    continue;
                };
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                match done.get(child) {
                    None if resource(child.as_str()).is_some() => {
                        done.insert(child, false);
                        stack.push((child, 0));
                    }
                    Some(false) => {
                        let start = stack.iter().position(|(s, _)| *s == child).unwrap_or(0);
                        let path: Vec<_> = stack[start..]
                            .iter()
                            .map(|(s, _)| s.as_str())
                            .chain(std::iter::once(child.as_str()))
                            .collect();
                        let location = document.shapes[id.as_str()]
                            .source
                            .child(&format!("/resources/{index}/target"));
                        self.error(
                            DiagnosticCode::InvalidShape,
                            format!("resource containment cycle: {}", path.join(" -> ")),
                            location,
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use crate::prelude::prelude;
    use DiagnosticCode::*;

    fn check_with(loader: ModelLoader, shapes: &str) -> Vec<(DiagnosticCode, String)> {
        let json = format!(r#"{{"smithy":"2.0","shapes":{{{shapes}}}}}"#);
        match loader.load_str("t.json", &json) {
            Ok(_) => Vec::new(),
            Err(err) => {
                assert!(err.is_invalid_model(), "{err}");
                err.diagnostics()
                    .iter()
                    .map(|d| {
                        let pointer = d.source_location().and_then(|l| l.pointer());
                        (d.code(), pointer.unwrap_or_default().to_owned())
                    })
                    .collect()
            }
        }
    }

    fn check(shapes: &str) -> Vec<(DiagnosticCode, String)> {
        check_with(ModelLoader::new(), shapes)
    }

    fn one(code: DiagnosticCode, pointer: &str) -> Vec<(DiagnosticCode, String)> {
        vec![(code, pointer.to_owned())]
    }

    const STR: &str = r#""a#Str":{"type":"string"}"#;

    #[test]
    fn prelude_passes_structural_v1() {
        let options = ModelLoader::new().require_trait_definitions(true);
        let diagnostics = validate(prelude(), None, &options);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn valid_model() {
        let shapes = r#"
            "a#Svc":{"type":"service","operations":[{"target":"a#Op"}],"resources":[{"target":"a#R"}],
                     "errors":[{"target":"a#Err"}],"rename":{"a#Str":"Name","a#In":"Input2"}},
            "a#Op":{"type":"operation","input":{"target":"a#In"},"output":{"target":"smithy.api#Unit"},
                    "errors":[{"target":"a#Err"}]},
            "a#In":{"type":"structure","members":{"s":{"target":"a#Str"},"m":{"target":"a#M"}}},
            "a#Err":{"type":"structure","traits":{"smithy.api#error":"server"}},
            "a#Str":{"type":"string","traits":{"smithy.api#documentation":"d","a#undefined":{}}},
            "a#M":{"type":"map","key":{"target":"a#E"},"value":{"target":"a#U"}},
            "a#E":{"type":"enum","members":{"A":{"target":"smithy.api#Unit"},
                   "B":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"b"}}}},
            "a#I":{"type":"intEnum","members":{"A":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":-1}}}},
            "a#U":{"type":"union","members":{"x":{"target":"smithy.api#Unit"},"y":{"target":"a#In"}}},
            "a#R":{"type":"resource","identifiers":{"id":{"target":"a#Str"}},"properties":{"p":{"target":"a#In"}},
                   "read":{"target":"a#Op"},"operations":[{"target":"a#Op"}],
                   "collectionOperations":[{"target":"a#Op"}],"resources":[{"target":"a#Child"}]},
            "a#Child":{"type":"resource"},
            "a#t":{"type":"structure","traits":{"smithy.api#trait":{}}},
            "a#Tagged":{"type":"string","traits":{"a#t":{}}}
        "#;
        assert_eq!(check(shapes), []);
    }

    #[test]
    fn unresolved_and_member_references() {
        assert_eq!(
            check(r#""a#S":{"type":"structure","members":{"m":{"target":"a#Missing"}}}"#),
            one(UnresolvedReference, "/shapes/a#S/members/m/target")
        );
        assert_eq!(
            check(&format!(
                r#"{STR},"a#S":{{"type":"structure","members":{{"m":{{"target":"a#Str"}}}}}},
                   "a#L":{{"type":"list","member":{{"target":"a#S$m"}}}}"#
            )),
            one(InvalidTarget, "/shapes/a#L/member/target")
        );
        assert_eq!(
            check_with(
                ModelLoader::new().disable_prelude(),
                r#""a#L":{"type":"list","member":{"target":"smithy.api#String"}}"#
            ),
            one(UnresolvedReference, "/shapes/a#L/member/target")
        );
    }

    #[test]
    fn members_target_data_shapes() {
        assert_eq!(
            check(
                r#""a#Op":{"type":"operation"},"a#L":{"type":"list","member":{"target":"a#Op"}}"#
            ),
            one(InvalidTarget, "/shapes/a#L/member/target")
        );
        assert_eq!(
            check(
                r#""a#t":{"type":"structure","traits":{"smithy.api#trait":{}}},
                   "a#L":{"type":"list","member":{"target":"a#t"}}"#
            ),
            one(InvalidTarget, "/shapes/a#L/member/target")
        );
    }

    #[test]
    fn trait_definitions() {
        let shapes = format!(
            r#"{STR},"a#S":{{"type":"structure","members":{{"m":{{"target":"a#Str","traits":{{"a#Str":{{}}}}}}}},
                     "traits":{{"a#Str":{{}}}}}}"#
        );
        assert_eq!(
            check(&shapes),
            [
                (InvalidTrait, "/shapes/a#S/traits/a#Str".to_owned()),
                (
                    InvalidTrait,
                    "/shapes/a#S/members/m/traits/a#Str".to_owned()
                ),
            ]
        );
        let unknown =
            r#""a#S":{"type":"string","traits":{"x#unknown":{},"smithy.api#sensitive":{}}}"#;
        assert_eq!(check(unknown), []);
        assert_eq!(
            check_with(ModelLoader::new().require_trait_definitions(true), unknown),
            one(InvalidTrait, "/shapes/a#S/traits/x#unknown")
        );
    }

    #[test]
    fn map_keys() {
        assert_eq!(
            check(
                r#""a#M":{"type":"map","key":{"target":"smithy.api#Integer"},"value":{"target":"smithy.api#Integer"}}"#
            ),
            one(InvalidTarget, "/shapes/a#M/key/target")
        );
        assert_eq!(
            check(
                r#""a#M":{"type":"map","key":{"target":"smithy.api#String"},"value":{"target":"smithy.api#String"}}"#
            ),
            []
        );
    }

    #[test]
    fn case_conflicts() {
        let err = ModelLoader::new()
            .load_str(
                "t",
                r#"{"smithy":"2.0","shapes":{"a#S":{"type":"structure","members":{
                    "foo":{"target":"smithy.api#String"},"Foo":{"target":"smithy.api#String"}}}}}"#,
            )
            .unwrap_err();
        let d = err.diagnostics().iter().next().unwrap();
        assert_eq!(d.code(), CaseConflict);
        assert_eq!(
            d.source_location().unwrap().pointer(),
            Some("/shapes/a#S/members/Foo")
        );
        assert_eq!(d.related()[0].pointer(), Some("/shapes/a#S/members/foo"));

        assert_eq!(
            check(r#""a#Foo":{"type":"string"},"a#foo":{"type":"string"}"#),
            one(CaseConflict, "/shapes/a#foo")
        );
        // A different-case namespace passes the loader's prelude check but conflicts here.
        assert_eq!(
            check(r#""smithy.API#string":{"type":"string"}"#),
            one(CaseConflict, "/shapes/smithy.API#string")
        );
        assert_eq!(
            check(
                r#""a#U":{"type":"union","members":{"a":{"target":"smithy.api#String"},"A":{"target":"smithy.api#String"}}}"#
            ),
            one(CaseConflict, "/shapes/a#U/members/A")
        );
    }

    #[test]
    fn unions_and_enums() {
        assert_eq!(
            check(r#""a#U":{"type":"union"}"#),
            one(InvalidShape, "/shapes/a#U")
        );
        assert_eq!(
            check(r#""a#E":{"type":"enum","members":{}}"#),
            one(InvalidShape, "/shapes/a#E")
        );
        assert_eq!(
            check(r#""a#E":{"type":"intEnum"}"#),
            one(InvalidShape, "/shapes/a#E")
        );
        assert_eq!(
            check(r#""a#E":{"type":"enum","members":{"A":{"target":"smithy.api#String"}}}"#),
            one(InvalidTarget, "/shapes/a#E/members/A/target")
        );
        assert_eq!(
            check(
                r#""a#E":{"type":"enum","members":{"A":{"target":"smithy.api#Unit"},
                "B":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"A"}}}}"#
            ),
            one(InvalidShape, "/shapes/a#E/members/B")
        );
        for bad in [r#"1"#, r#""""#] {
            assert_eq!(
                check(&format!(
                    r#""a#E":{{"type":"enum","members":{{"A":{{"target":"smithy.api#Unit","traits":{{"smithy.api#enumValue":{bad}}}}}}}}}"#
                )),
                one(
                    InvalidTrait,
                    "/shapes/a#E/members/A/traits/smithy.api#enumValue"
                )
            );
        }
        assert_eq!(
            check(r#""a#I":{"type":"intEnum","members":{"A":{"target":"smithy.api#Unit"}}}"#),
            one(InvalidShape, "/shapes/a#I/members/A")
        );
        for bad in ["1.5", "\"1\"", "2147483648"] {
            assert_eq!(
                check(&format!(
                    r#""a#I":{{"type":"intEnum","members":{{"A":{{"target":"smithy.api#Unit","traits":{{"smithy.api#enumValue":{bad}}}}}}}}}"#
                )),
                one(
                    InvalidTrait,
                    "/shapes/a#I/members/A/traits/smithy.api#enumValue"
                ),
                "{bad}"
            );
        }
        assert_eq!(
            check(
                r#""a#I":{"type":"intEnum","members":{
                "A":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":1}},
                "B":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":1}}}}"#
            ),
            one(InvalidShape, "/shapes/a#I/members/B")
        );
    }

    #[test]
    fn unit_targets() {
        assert_eq!(
            check(r#""a#S":{"type":"structure","members":{"m":{"target":"smithy.api#Unit"}}}"#),
            one(InvalidTarget, "/shapes/a#S/members/m/target")
        );
        assert_eq!(
            check(r#""a#L":{"type":"list","member":{"target":"smithy.api#Unit"}}"#),
            one(InvalidTarget, "/shapes/a#L/member/target")
        );
        assert_eq!(
            check(r#""a#R":{"type":"resource","properties":{"p":{"target":"smithy.api#Unit"}}}"#),
            one(InvalidTarget, "/shapes/a#R/properties/p/target")
        );
    }

    #[test]
    fn services() {
        let shapes = format!(
            r#"{STR},"a#S":{{"type":"structure"}},
               "a#Svc":{{"type":"service","operations":[{{"target":"a#S"}}],"resources":[{{"target":"a#S"}}],
                       "errors":[{{"target":"a#S"}},{{"target":"a#Str"}}],
                       "rename":{{"a#Missing":"X","a#S":"not-valid","a#Str":"Dup"}}}},
               "a#Svc2":{{"type":"service","rename":{{"a#S":"Dup","a#Str":"dup"}}}}"#
        );
        assert_eq!(
            check(&shapes),
            [
                (
                    InvalidTarget,
                    "/shapes/a#Svc/operations/0/target".to_owned()
                ),
                (InvalidTarget, "/shapes/a#Svc/resources/0/target".to_owned()),
                (InvalidTarget, "/shapes/a#Svc/errors/0/target".to_owned()),
                (InvalidTarget, "/shapes/a#Svc/errors/1/target".to_owned()),
                (
                    UnresolvedReference,
                    "/shapes/a#Svc/rename/a#Missing".to_owned()
                ),
                (InvalidProperty, "/shapes/a#Svc/rename/a#S".to_owned()),
                (CaseConflict, "/shapes/a#Svc2/rename/a#Str".to_owned()),
            ]
        );
    }

    #[test]
    fn operations() {
        let shapes = format!(
            r#"{STR},"a#S":{{"type":"structure"}},
               "a#Op":{{"type":"operation","input":{{"target":"a#Str"}},"output":{{"target":"a#Op"}},
                       "errors":[{{"target":"a#S"}}]}}"#
        );
        assert_eq!(
            check(&shapes),
            [
                (InvalidTarget, "/shapes/a#Op/input/target".to_owned()),
                (InvalidTarget, "/shapes/a#Op/output/target".to_owned()),
                (InvalidTarget, "/shapes/a#Op/errors/0/target".to_owned()),
            ]
        );
    }

    #[test]
    fn omitted_operation_io_requires_unit() {
        let without_prelude = ModelLoader::new().disable_prelude();
        assert_eq!(
            check_with(without_prelude.clone(), r#""a#Op":{"type":"operation"}"#),
            [
                (UnresolvedReference, "/shapes/a#Op".to_owned()),
                (UnresolvedReference, "/shapes/a#Op".to_owned()),
            ]
        );
        // Defining Unit locally makes the normalized reference resolve.
        let unit = r#""smithy.api#Unit":{"type":"structure","traits":{"smithy.api#unitType":{}}},
                      "a#Op":{"type":"operation"}"#;
        assert_eq!(check_with(without_prelude, unit), []);
        assert_eq!(check(r#""a#Op":{"type":"operation"}"#), []);
    }

    #[test]
    fn resources() {
        let shapes = r#""a#S":{"type":"structure"},"a#Op":{"type":"operation"},
            "a#R":{"type":"resource","identifiers":{"id":{"target":"smithy.api#Integer"}},
                   "properties":{"p":{"target":"a#Op"}},"read":{"target":"a#S"},
                   "operations":[{"target":"a#S"}],"collectionOperations":[{"target":"a#S"}],
                   "resources":[{"target":"a#Op"}]}"#;
        assert_eq!(
            check(shapes),
            [
                (
                    InvalidTarget,
                    "/shapes/a#R/identifiers/id/target".to_owned()
                ),
                (InvalidTarget, "/shapes/a#R/properties/p/target".to_owned()),
                (InvalidTarget, "/shapes/a#R/read/target".to_owned()),
                (InvalidTarget, "/shapes/a#R/operations/0/target".to_owned()),
                (
                    InvalidTarget,
                    "/shapes/a#R/collectionOperations/0/target".to_owned()
                ),
                (InvalidTarget, "/shapes/a#R/resources/0/target".to_owned()),
            ]
        );
    }

    #[test]
    fn resource_cycles() {
        assert_eq!(
            check(
                r#""a#A":{"type":"resource","resources":[{"target":"a#B"}]},
                   "a#B":{"type":"resource","resources":[{"target":"a#C"}]},
                   "a#C":{"type":"resource","resources":[{"target":"a#A"}]}"#
            ),
            one(InvalidShape, "/shapes/a#C/resources/0/target")
        );
        let err = Model::from_json_str(
            "t",
            r#"{"smithy":"2.0","shapes":{"a#A":{"type":"resource","resources":[{"target":"a#A"}]}}}"#,
        )
        .unwrap_err();
        let message = err
            .diagnostics()
            .iter()
            .next()
            .unwrap()
            .message()
            .to_owned();
        assert_eq!(message, "resource containment cycle: a#A -> a#A");
        // A diamond is not a cycle.
        assert_eq!(
            check(
                r#""a#A":{"type":"resource","resources":[{"target":"a#B"},{"target":"a#C"}]},
                   "a#B":{"type":"resource","resources":[{"target":"a#D"}]},
                   "a#C":{"type":"resource","resources":[{"target":"a#D"}]},
                   "a#D":{"type":"resource"}"#
            ),
            []
        );
    }
}
