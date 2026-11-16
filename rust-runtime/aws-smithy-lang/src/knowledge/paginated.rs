/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Service-scoped pagination of operations.

use super::{OperationIndex, TopDownIndex};
use crate::diagnostic::{fmt_located, SourceLocation};
use crate::node::Node;
use crate::relationships::RelationshipIndex;
use crate::shape::{MemberView, OperationView, ServiceView, StructureView};
use crate::shape_id::ShapeId;
use crate::traits::{self, Paginated, TraitDecodeError};
use std::fmt;

/// The pagination of operations within a service, resolved to the members it names.
///
/// An operation is paginated within a service when the service contains it, directly or
/// through resources, and the operation has its own `@paginated` trait. Properties that the
/// operation's trait omits come from the service's `@paginated` trait, so one operation can
/// paginate differently in two services. A service's trait alone does not make an operation
/// paginated.
///
/// `inputToken` and `pageSize` name top-level input members. `outputToken` and `items` are
/// paths of output member names separated by `.`, and every member before the last must
/// target a structure. [`PaginationInfo`] keeps every member of each path.
///
/// The index checks only what resolution needs: well-formed trait values, an effective
/// `inputToken` and `outputToken`, and members that exist. It does not check member targets
/// or whether members are required. Pagination that cannot be resolved is a
/// [`PaginationIndexError`] rather than `None`. (The reference implementation's index omits
/// such pagination, relying on model validation to have rejected it.)
///
/// Answers are computed per query from the cached relationship index.
///
/// ```
/// use aws_smithy_lang::{Model, ShapeId};
///
/// let model = Model::from_idl_str("things.smithy", r#"
/// $version: "2"
/// namespace ex
///
/// @paginated(inputToken: "nextToken", outputToken: "nextToken", pageSize: "maxResults")
/// service Things { operations: [ListThings] }
///
/// @readonly
/// @paginated(items: "page.things")
/// operation ListThings {
///     input := { nextToken: String, maxResults: Integer }
///     output := { nextToken: String, page: Page }
/// }
///
/// structure Page { things: ThingList }
/// list ThingList { member: String }
/// "#)?;
///
/// let info = model
///     .knowledge()
///     .paginated()
///     .get(&ShapeId::from_static("ex#Things"), &ShapeId::from_static("ex#ListThings"))?
///     .expect("ListThings is paginated");
///
/// // The tokens and page size come from the service; `items` comes from the operation.
/// assert_eq!(info.input_token_member().id(), "ex#ListThingsInput$nextToken");
/// assert_eq!(info.page_size_member().unwrap().name(), "maxResults");
/// assert_eq!(info.paginated_trait().items(), Some("page.things"));
///
/// // Paths keep every member, from the output structure's member to the last.
/// let items: Vec<_> = info.items_member_path().iter().map(|m| m.id().to_string()).collect();
/// assert_eq!(items, ["ex#ListThingsOutput$page", "ex#Page$things"]);
/// # Ok::<_, Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy)]
pub struct PaginatedIndex<'m> {
    relationships: RelationshipIndex<'m>,
}

impl<'m> PaginatedIndex<'m> {
    pub(crate) fn new(relationships: RelationshipIndex<'m>) -> Self {
        Self { relationships }
    }

    /// The pagination of `operation` within `service`.
    ///
    /// Returns `Ok(None)` if `service` is not a service, `operation` is not an operation, the
    /// service does not contain the operation, or the operation has no `@paginated` trait.
    ///
    /// Returns an error if the operation's or the service's `@paginated` trait is malformed,
    /// if neither sets `inputToken` or `outputToken`, or if a member name or path that they
    /// set does not resolve.
    pub fn get(
        &self,
        service: &ShapeId,
        operation: &ShapeId,
    ) -> Result<Option<PaginationInfo<'m>>, PaginationIndexError> {
        let Some(knowledge) = OperationIndex::new(self.relationships).get(operation) else {
            return Ok(None);
        };
        let Some((service_node, service)) = self.relationships.lookup(service) else {
            return Ok(None);
        };
        let Some(service) = service.as_service() else {
            return Ok(None);
        };
        let operation = knowledge.operation();
        // Checked before containment because it is cheaper.
        let Some(operation_trait) = operation.get_trait(&traits::PAGINATED) else {
            return Ok(None);
        };
        if !TopDownIndex::new(self.relationships).contains(service_node, knowledge.node) {
            return Ok(None);
        }

        let declared = |owner: &'m ShapeId, node: &'m Node| {
            traits::decode_value::<Paginated>(node)
                .map(|value| Declared { owner, node, value })
                .map_err(|err| {
                    let location = err.source_location().clone();
                    PaginationIndexError::new(
                        service.id(),
                        operation.id(),
                        owner,
                        location,
                        ErrorKind::InvalidTrait(err),
                    )
                })
        };
        let lookup = Lookup {
            service,
            operation,
            operation_trait: declared(operation.id(), operation_trait)?,
            service_trait: service
                .get_trait(&traits::PAGINATED)
                .map(|node| declared(service.id(), node))
                .transpose()?,
        };

        let input = knowledge.input_shape();
        let output = knowledge.output_shape();
        let input_token = lookup.input_member(input, &lookup.required(INPUT_TOKEN)?)?;
        let output_token = lookup.output_path(output, &lookup.required(OUTPUT_TOKEN)?)?;
        let page_size = match lookup.setting(PAGE_SIZE) {
            Some(setting) => Some(lookup.input_member(input, &setting)?),
            None => None,
        };
        let items = match lookup.setting(ITEMS) {
            Some(setting) => lookup.output_path(output, &setting)?,
            None => Box::default(),
        };
        let paginated = lookup
            .operation_trait
            .value
            .with_defaults_from(lookup.service_trait.as_ref().map(|d| &d.value));
        Ok(Some(PaginationInfo {
            service,
            operation,
            input,
            output,
            paginated,
            input_token,
            output_token,
            page_size,
            items,
        }))
    }
}

impl fmt::Debug for PaginatedIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaginatedIndex").finish_non_exhaustive()
    }
}

/// The resolved pagination of one operation within one service, returned by
/// [`PaginatedIndex::get`].
#[derive(Clone, Debug)]
pub struct PaginationInfo<'m> {
    service: ServiceView<'m>,
    operation: OperationView<'m>,
    input: StructureView<'m>,
    output: StructureView<'m>,
    paginated: Paginated,
    input_token: MemberView<'m>,
    output_token: Box<[MemberView<'m>]>,
    page_size: Option<MemberView<'m>>,
    items: Box<[MemberView<'m>]>,
}

impl<'m> PaginationInfo<'m> {
    /// The service the pagination applies within.
    pub fn service(&self) -> ServiceView<'m> {
        self.service
    }

    /// The paginated operation.
    pub fn operation(&self) -> OperationView<'m> {
        self.operation
    }

    /// The operation's input structure.
    pub fn input_shape(&self) -> StructureView<'m> {
        self.input
    }

    /// The operation's output structure.
    pub fn output_shape(&self) -> StructureView<'m> {
        self.output
    }

    /// The effective trait: the operation's `@paginated` values, with each property it omits
    /// taken from the service's `@paginated` trait.
    pub fn paginated_trait(&self) -> &Paginated {
        &self.paginated
    }

    /// The input member named by `inputToken`.
    pub fn input_token_member(&self) -> MemberView<'m> {
        self.input_token
    }

    /// The members named by the `outputToken` path, from a member of the output structure to
    /// the token itself. Never empty.
    pub fn output_token_member_path(&self) -> &[MemberView<'m>] {
        &self.output_token
    }

    /// The input member named by `pageSize`, or `None` if the effective trait omits it.
    pub fn page_size_member(&self) -> Option<MemberView<'m>> {
        self.page_size
    }

    /// The members named by the `items` path, from a member of the output structure to the
    /// paginated results. Empty if the effective trait omits `items`.
    pub fn items_member_path(&self) -> &[MemberView<'m>] {
        &self.items
    }
}

/// The pagination of an operation within a service cannot be resolved.
///
/// [`trait_owner`](Self::trait_owner) and [`source_location`](Self::source_location) identify
/// the value at fault: a malformed trait value, the operation's `@paginated` trait when a
/// required property is missing, or the property that does not resolve. A property inherited
/// from the service is reported at the service's trait.
#[derive(Debug)]
pub struct PaginationIndexError(Box<ErrorData>);

/// Boxed so that results carrying the error stay small.
#[derive(Debug)]
struct ErrorData {
    service: ShapeId,
    operation: ShapeId,
    trait_owner: ShapeId,
    location: SourceLocation,
    kind: ErrorKind,
}

#[derive(Debug)]
enum ErrorKind {
    /// A `@paginated` trait value could not be decoded.
    InvalidTrait(TraitDecodeError),
    /// Neither the operation's nor the service's trait sets a required property.
    MissingProperty { property: &'static str },
    /// A property's member name or path does not resolve.
    Unresolved {
        property: &'static str,
        value: String,
        reason: Unresolved,
    },
}

#[derive(Debug)]
enum Unresolved {
    /// The input structure has no member with the property's name.
    NotAnInputMember { input: ShapeId },
    /// A path contains an empty member name.
    EmptySegment,
    /// A path's member name is not a member of the structure reached so far.
    NotAMember { segment: String, container: ShapeId },
    /// A path continues past a member that does not target a structure.
    NotAStructure { member: ShapeId, target: ShapeId },
}

impl PaginationIndexError {
    fn new(
        service: &ShapeId,
        operation: &ShapeId,
        trait_owner: &ShapeId,
        location: SourceLocation,
        kind: ErrorKind,
    ) -> Self {
        Self(Box::new(ErrorData {
            service: service.clone(),
            operation: operation.clone(),
            trait_owner: trait_owner.clone(),
            location,
            kind,
        }))
    }

    /// The service the query was scoped to.
    pub fn service(&self) -> &ShapeId {
        &self.0.service
    }

    /// The queried operation.
    pub fn operation(&self) -> &ShapeId {
        &self.0.operation
    }

    /// The operation or service whose `@paginated` trait is at fault: the shape with a
    /// malformed trait, the operation when a required property is missing, or the shape whose
    /// trait sets the property that does not resolve.
    pub fn trait_owner(&self) -> &ShapeId {
        &self.0.trait_owner
    }

    /// The `@paginated` property at fault, such as `outputToken`, or `None` for a malformed
    /// trait value.
    pub fn property(&self) -> Option<&str> {
        match &self.0.kind {
            ErrorKind::InvalidTrait(_) => None,
            ErrorKind::MissingProperty { property } | ErrorKind::Unresolved { property, .. } => {
                Some(property)
            }
        }
    }

    /// The member name or path that does not resolve, or `None` for a malformed trait value
    /// or a missing property.
    pub fn value(&self) -> Option<&str> {
        match &self.0.kind {
            ErrorKind::Unresolved { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Where the value at fault was loaded from, or [`SourceLocation::none`] for values
    /// created in code.
    pub fn source_location(&self) -> &SourceLocation {
        &self.0.location
    }
}

impl fmt::Display for PaginationIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let error = &self.0;
        fmt_located(
            f,
            &error.location,
            format_args!(
                "cannot resolve the pagination of `{}` in service `{}`: {}",
                error.operation,
                error.service,
                Reason(error)
            ),
        )
    }
}

/// The part of an error message that follows the query.
struct Reason<'a>(&'a ErrorData);

impl fmt::Display for Reason<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let error = self.0;
        let (property, value, reason) = match &error.kind {
            ErrorKind::InvalidTrait(err) => {
                return write!(
                    f,
                    "invalid `@paginated` trait on `{}`: {}",
                    error.trait_owner,
                    err.message()
                );
            }
            ErrorKind::MissingProperty { property } => {
                return write!(
                    f,
                    "neither the operation's nor the service's `@paginated` trait sets `{property}`"
                );
            }
            ErrorKind::Unresolved {
                property,
                value,
                reason,
            } => (property, value, reason),
        };
        write!(f, "`{property}`")?;
        if error.trait_owner != error.operation {
            write!(f, " (inherited from `{}`)", error.trait_owner)?;
        }
        match reason {
            Unresolved::NotAnInputMember { input } => {
                write!(f, " names `{value}`, which is not a member of `{input}`")
            }
            Unresolved::EmptySegment => write!(f, " path `{value}` has an empty member name"),
            Unresolved::NotAMember { segment, container } => write!(
                f,
                " path `{value}`: `{segment}` is not a member of `{container}`"
            ),
            Unresolved::NotAStructure { member, target } => write!(
                f,
                " path `{value}`: `{member}` targets `{target}`, which is not a structure"
            ),
        }
    }
}

impl std::error::Error for PaginationIndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.0.kind {
            ErrorKind::InvalidTrait(err) => Some(err),
            _ => None,
        }
    }
}

/// A `@paginated` property and its accessor.
#[derive(Clone, Copy)]
struct Property {
    name: &'static str,
    get: fn(&Paginated) -> Option<&str>,
}

const INPUT_TOKEN: Property = Property {
    name: Paginated::INPUT_TOKEN,
    get: Paginated::input_token,
};
const OUTPUT_TOKEN: Property = Property {
    name: Paginated::OUTPUT_TOKEN,
    get: Paginated::output_token,
};
const ITEMS: Property = Property {
    name: Paginated::ITEMS,
    get: Paginated::items,
};
const PAGE_SIZE: Property = Property {
    name: Paginated::PAGE_SIZE,
    get: Paginated::page_size,
};

/// A decoded `@paginated` trait and the shape it is applied to.
struct Declared<'m> {
    owner: &'m ShapeId,
    node: &'m Node,
    value: Paginated,
}

impl<'m> Declared<'m> {
    /// Where `property` is set, falling back to the trait value.
    fn location_of(&self, property: &str) -> &'m SourceLocation {
        self.node
            .as_object()
            .and_then(|object| object.get(property))
            .unwrap_or(self.node)
            .source_location()
    }
}

/// An effective property value and the trait that sets it.
struct Setting<'a, 'm> {
    property: &'static str,
    value: &'a str,
    declared: &'a Declared<'m>,
}

/// A query for an operation that is paginated within the service.
struct Lookup<'m> {
    service: ServiceView<'m>,
    operation: OperationView<'m>,
    operation_trait: Declared<'m>,
    service_trait: Option<Declared<'m>>,
}

impl<'m> Lookup<'m> {
    /// The effective value of `property`: the operation's, then the service's.
    fn setting(&self, property: Property) -> Option<Setting<'_, 'm>> {
        [Some(&self.operation_trait), self.service_trait.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|declared| {
                Some(Setting {
                    property: property.name,
                    value: (property.get)(&declared.value)?,
                    declared,
                })
            })
    }

    fn required(&self, property: Property) -> Result<Setting<'_, 'm>, PaginationIndexError> {
        self.setting(property).ok_or_else(|| {
            self.error(
                &self.operation_trait,
                self.operation_trait.node.source_location(),
                ErrorKind::MissingProperty {
                    property: property.name,
                },
            )
        })
    }

    fn input_member(
        &self,
        input: StructureView<'m>,
        setting: &Setting<'_, 'm>,
    ) -> Result<MemberView<'m>, PaginationIndexError> {
        input.member(setting.value).ok_or_else(|| {
            self.unresolved(
                setting,
                Unresolved::NotAnInputMember {
                    input: input.id().clone(),
                },
            )
        })
    }

    fn output_path(
        &self,
        output: StructureView<'m>,
        setting: &Setting<'_, 'm>,
    ) -> Result<Box<[MemberView<'m>]>, PaginationIndexError> {
        resolve_path(output, setting.value).map_err(|reason| self.unresolved(setting, reason))
    }

    fn unresolved(&self, setting: &Setting<'_, 'm>, reason: Unresolved) -> PaginationIndexError {
        self.error(
            setting.declared,
            setting.declared.location_of(setting.property),
            ErrorKind::Unresolved {
                property: setting.property,
                value: setting.value.to_owned(),
                reason,
            },
        )
    }

    fn error(
        &self,
        declared: &Declared<'m>,
        location: &SourceLocation,
        kind: ErrorKind,
    ) -> PaginationIndexError {
        PaginationIndexError::new(
            self.service.id(),
            self.operation.id(),
            declared.owner,
            location.clone(),
            kind,
        )
    }
}

/// Resolves a path of member names separated by `.`, starting at `root`, keeping every
/// member. The last member may target any shape.
fn resolve_path<'m>(
    root: StructureView<'m>,
    path: &str,
) -> Result<Box<[MemberView<'m>]>, Unresolved> {
    let mut members: Vec<MemberView<'m>> = Vec::new();
    for segment in path.split('.') {
        // Checked first: `token.` has an empty name, whatever `token` targets.
        if segment.is_empty() {
            return Err(Unresolved::EmptySegment);
        }
        let container =
            match members.last() {
                None => root,
                Some(member) => member.target_shape().as_structure().ok_or_else(|| {
                    Unresolved::NotAStructure {
                        member: member.id().clone(),
                        target: member.target().clone(),
                    }
                })?,
            };
        let member = container
            .member(segment)
            .ok_or_else(|| Unresolved::NotAMember {
                segment: segment.to_owned(),
                container: container.id().clone(),
            })?;
        members.push(member);
    }
    Ok(members.into_boxed_slice())
}
