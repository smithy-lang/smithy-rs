/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The built-in `StructuralV1` validators.
//!
//! Each validator checks the model's own shapes (the embedded prelude is trusted; a unit test
//! checks that it passes) through the same public views a custom validator uses. The rules that
//! make views infallible run earlier, as critical checks in the assembler.

use super::{DiagnosticSink, Validator};
use crate::diagnostic::{escape_pointer_token, Diagnostic, DiagnosticCode, SourceLocation};
use crate::model::Model;
use crate::prelude;
use crate::shape::{MemberView, ShapeType, ShapeView};
use crate::shape_id::ShapeId;
use crate::traits;
use std::collections::HashMap;
use std::sync::Arc;

/// The built-in validators, in the order they run.
pub(crate) fn builtin_validators(require_trait_definitions: bool) -> Vec<Arc<dyn Validator>> {
    vec![
        Arc::new(CaseConflict),
        Arc::new(TraitDefinition {
            require_definitions: require_trait_definitions,
        }),
        Arc::new(MapKey),
        Arc::new(UnionMembers),
        Arc::new(EnumMembers),
        Arc::new(UnitTarget),
        Arc::new(ErrorTarget),
        Arc::new(ServiceRename),
    ]
}

fn error(
    code: DiagnosticCode,
    message: String,
    id: &ShapeId,
    location: &SourceLocation,
) -> Diagnostic {
    Diagnostic::error(code, message)
        .with_shape_id(id.clone())
        .with_source_location(location.clone())
}

fn own_shapes(model: &Model) -> impl Iterator<Item = ShapeView<'_>> {
    model.non_prelude_shapes()
}

fn conflict(
    message: String,
    id: &ShapeId,
    location: &SourceLocation,
    first: &SourceLocation,
) -> Diagnostic {
    error(DiagnosticCode::CaseConflict, message, id, location).with_related(first.clone())
}

/// Shape IDs, and the members of each shape, are case-insensitively unique.
struct CaseConflict;

impl Validator for CaseConflict {
    fn name(&self) -> &str {
        "CaseConflict"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        let mut seen: HashMap<String, ShapeView<'_>> = HashMap::new();
        // Prelude first, so a conflict is reported on the model's own shape.
        for shape in model.prelude_shapes().chain(own_shapes(model)) {
            let key = shape.id().as_str().to_ascii_lowercase();
            match seen.get(&key) {
                Some(first) => diagnostics.push(conflict(
                    format!(
                        "`{}` conflicts case-insensitively with `{}`",
                        shape.id(),
                        first.id()
                    ),
                    shape.id(),
                    shape.source_location(),
                    first.source_location(),
                )),
                None => {
                    seen.insert(key, shape);
                }
            }
        }
        for shape in own_shapes(model) {
            let mut members: HashMap<String, MemberView<'_>> = HashMap::new();
            for member in shape.members().iter() {
                let key = member.name().to_ascii_lowercase();
                match members.get(&key) {
                    Some(first) => diagnostics.push(conflict(
                        format!(
                            "member `{}` of `{}` conflicts case-insensitively with `{}`",
                            member.name(),
                            shape.id(),
                            first.name()
                        ),
                        member.id(),
                        member.source_location(),
                        first.source_location(),
                    )),
                    None => {
                        members.insert(key, member);
                    }
                }
            }
        }
    }
}

/// An applied trait that resolves to a shape resolves to a trait definition. Missing
/// definitions are errors only with `require_trait_definitions`.
struct TraitDefinition {
    require_definitions: bool,
}

impl TraitDefinition {
    fn check(
        &self,
        model: &Model,
        owner: &ShapeId,
        applied: crate::shape::Traits<'_>,
        diagnostics: &mut DiagnosticSink<'_>,
    ) {
        for applied in applied.iter() {
            let id = applied.id();
            let location = applied.source_location();
            match model.get_shape(id) {
                Some(definition) if !definition.has_trait(&traits::TRAIT) => {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidTrait,
                        format!("`{id}` is applied as a trait but is not a trait definition"),
                        owner,
                        location,
                    ))
                }
                Some(_) => {}
                None if self.require_definitions => diagnostics.push(error(
                    DiagnosticCode::InvalidTrait,
                    format!("trait `{id}` has no definition"),
                    owner,
                    location,
                )),
                None => {}
            }
        }
    }
}

impl Validator for TraitDefinition {
    fn name(&self) -> &str {
        "TraitDefinition"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        for shape in own_shapes(model) {
            self.check(model, shape.id(), shape.traits(), diagnostics);
            for member in shape.members().iter() {
                self.check(model, member.id(), member.traits(), diagnostics);
            }
        }
    }
}

/// Map keys target `string` or `enum`.
struct MapKey;

impl Validator for MapKey {
    fn name(&self) -> &str {
        "MapKey"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        for map in own_shapes(model).filter_map(|s| s.as_map()) {
            let key = map.key();
            let target = key.target_shape();
            // A Unit key is reported by `UnitTarget`.
            if !matches!(target.shape_type(), ShapeType::String | ShapeType::Enum)
                && key.target() != prelude::unit_id()
            {
                diagnostics.push(error(
                    DiagnosticCode::InvalidTarget,
                    format!(
                        "map key `{}` must target a string or enum, but `{}` is a {}",
                        key.id(),
                        key.target(),
                        target.shape_type()
                    ),
                    key.id(),
                    &key.source_location().child("/target"),
                ));
            }
        }
    }
}

fn require_members(shape: ShapeView<'_>, diagnostics: &mut DiagnosticSink<'_>) {
    if shape.members().is_empty() {
        diagnostics.push(error(
            DiagnosticCode::InvalidShape,
            format!(
                "{} `{}` must have at least one member",
                shape.shape_type(),
                shape.id()
            ),
            shape.id(),
            shape.source_location(),
        ));
    }
}

/// Unions have at least one member.
struct UnionMembers;

impl Validator for UnionMembers {
    fn name(&self) -> &str {
        "UnionMembers"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        for shape in own_shapes(model).filter(|s| s.is_union()) {
            require_members(shape, diagnostics);
        }
    }
}

/// Enums and intEnums are non-empty, their members target `smithy.api#Unit`, and their
/// effective values are valid and unique.
struct EnumMembers;

impl Validator for EnumMembers {
    fn name(&self) -> &str {
        "EnumMembers"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        for shape in own_shapes(model).filter(|s| s.is_enum() || s.is_int_enum()) {
            let int_enum = shape.is_int_enum();
            require_members(shape, diagnostics);
            let mut seen: HashMap<String, MemberView<'_>> = HashMap::new();
            for member in shape.members().iter() {
                if member.target() != prelude::unit_id() {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidTarget,
                        format!(
                            "{} member `{}` must target `smithy.api#Unit`",
                            shape.shape_type(),
                            member.id()
                        ),
                        member.id(),
                        &member.source_location().child("/target"),
                    ));
                }
                let declared = member.get_trait(&traits::ENUM_VALUE);
                let value_location = || match declared {
                    Some(node) if !node.source_location().is_none() => {
                        node.source_location().clone()
                    }
                    _ => member
                        .source_location()
                        .child(&format!("/traits/{}", traits::ENUM_VALUE)),
                };
                let value = match (int_enum, declared) {
                    (false, None) => Some(member.name().to_owned()),
                    (false, Some(node)) if node.as_str().is_some_and(|v| !v.is_empty()) => {
                        node.as_str().map(str::to_owned)
                    }
                    (false, Some(_)) => {
                        diagnostics.push(error(
                            DiagnosticCode::InvalidTrait,
                            format!(
                                "enum member `{}` must have a non-empty string `@enumValue`",
                                member.id()
                            ),
                            member.id(),
                            &value_location(),
                        ));
                        None
                    }
                    (true, None) => {
                        diagnostics.push(error(
                            DiagnosticCode::InvalidShape,
                            format!(
                                "intEnum member `{}` requires an integer `@enumValue`",
                                member.id()
                            ),
                            member.id(),
                            member.source_location(),
                        ));
                        None
                    }
                    (true, Some(node)) => {
                        let value = node
                            .as_number()
                            .and_then(|n| n.as_i64())
                            .and_then(|n| i32::try_from(n).ok());
                        if value.is_none() {
                            diagnostics.push(error(
                                DiagnosticCode::InvalidTrait,
                                format!(
                                    "intEnum member `{}` must have a 32-bit integer `@enumValue`",
                                    member.id()
                                ),
                                member.id(),
                                &value_location(),
                            ));
                        }
                        value.map(|v| v.to_string())
                    }
                };
                let Some(value) = value else { continue };
                match seen.get(&value) {
                    Some(first) => diagnostics.push(
                        error(
                            DiagnosticCode::InvalidShape,
                            format!(
                                "`{}` members `{}` and `{}` have the same value `{value}`",
                                shape.id(),
                                first.name(),
                                member.name()
                            ),
                            member.id(),
                            member.source_location(),
                        )
                        .with_related(first.source_location().clone()),
                    ),
                    None => {
                        seen.insert(value, member);
                    }
                }
            }
        }
    }
}

/// `smithy.api#Unit` is only targeted by operation input/output and by union, enum, and
/// intEnum members.
struct UnitTarget;

impl Validator for UnitTarget {
    fn name(&self) -> &str {
        "UnitTarget"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        let unit = prelude::unit_id();
        for shape in own_shapes(model) {
            let allowed = matches!(
                shape.shape_type(),
                ShapeType::Union | ShapeType::Enum | ShapeType::IntEnum
            );
            if !allowed {
                for member in shape.members().iter().filter(|m| m.target() == unit) {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidTarget,
                        format!(
                            "`{}` cannot target `smithy.api#Unit`; only union, enum, and intEnum \
                             members and operation input/output can",
                            member.id()
                        ),
                        member.id(),
                        &member.source_location().child("/target"),
                    ));
                }
            }
            if let Some(resource) = shape.as_resource() {
                for (name, target) in resource.properties().filter(|(_, t)| *t == unit) {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidTarget,
                        format!(
                            "resource property `{name}` must target a data shape, but `{target}` \
                             is the unit type"
                        ),
                        shape.id(),
                        &shape.source_location().child(&format!(
                            "/properties/{}/target",
                            escape_pointer_token(name)
                        )),
                    ));
                }
            }
        }
    }
}

/// Service and operation `errors` target structures with the `@error` trait.
struct ErrorTarget;

impl Validator for ErrorTarget {
    fn name(&self) -> &str {
        "ErrorTarget"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        for shape in own_shapes(model) {
            let (errors, what): (Vec<&ShapeId>, _) = if let Some(service) = shape.as_service() {
                (service.common_errors().collect(), "service")
            } else if let Some(operation) = shape.as_operation() {
                (operation.direct_errors().collect(), "operation")
            } else {
                continue;
            };
            for (index, id) in errors.into_iter().enumerate() {
                let target = model
                    .expect_shape(id)
                    .expect("critical checks resolved errors");
                if !target.has_trait(&traits::ERROR) {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidTarget,
                        format!(
                            "{what} `errors` must target an error structure, but `{id}` has no \
                             `@error` trait"
                        ),
                        shape.id(),
                        &shape
                            .source_location()
                            .child(&format!("/errors/{index}/target")),
                    ));
                }
            }
        }
    }
}

/// Service `rename` keys resolve to root shapes, and values are valid, case-insensitively unique
/// identifiers. Full service-closure eligibility rules are not checked.
struct ServiceRename;

impl Validator for ServiceRename {
    fn name(&self) -> &str {
        "ServiceRename"
    }

    fn validate(&self, model: &Model, diagnostics: &mut DiagnosticSink<'_>) {
        for service in own_shapes(model).filter_map(|s| s.as_service()) {
            let mut names: HashMap<String, (&ShapeId, SourceLocation)> = HashMap::new();
            for (id, name) in service.rename() {
                let location = service
                    .source_location()
                    .child(&format!("/rename/{}", escape_pointer_token(id.as_str())));
                if id.is_member() || !model.contains_shape(id) {
                    diagnostics.push(error(
                        DiagnosticCode::UnresolvedReference,
                        format!("rename key `{id}` does not resolve to a root shape"),
                        service.id(),
                        &location,
                    ));
                }
                if !ShapeId::is_valid_identifier(name) {
                    diagnostics.push(error(
                        DiagnosticCode::InvalidProperty,
                        format!("rename of `{id}` to `{name}` is not a valid identifier"),
                        service.id(),
                        &location,
                    ));
                    continue;
                }
                match names.get(&name.to_ascii_lowercase()) {
                    Some((first, first_location)) => diagnostics.push(conflict(
                        format!(
                            "`{id}` and `{first}` are renamed to case-insensitively equal names"
                        ),
                        service.id(),
                        &location,
                        first_location,
                    )),
                    None => {
                        names.insert(name.to_ascii_lowercase(), (id, location));
                    }
                }
            }
        }
    }
}
