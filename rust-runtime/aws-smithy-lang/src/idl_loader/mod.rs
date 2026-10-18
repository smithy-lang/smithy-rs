/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Private Smithy 2.0 IDL loader.
//!
//! Loading has three steps, because a relative shape ID can refer to a declaration in any
//! other input of the same assembly:
//!
//! 1. [`load_slice`] validates and parses one input into an owned, unresolved syntax tree
//!    ([`syntax`]), stopping at the first syntax error like the JSON loader.
//! 2. The assembler indexes the declarations of every input in a
//!    [`DeclarationIndex`](resolve::DeclarationIndex).
//! 3. [`lower`](lower::lower) resolves shape IDs and IDL sugar and converts each shape through
//!    the same [`AstConverter`](crate::ast_converter::AstConverter) used by the JSON AST
//!    loader. The syntax tree is then dropped.

mod lexer;
pub(crate) mod lower;
mod node;
pub(crate) mod resolve;
pub(crate) mod syntax;

use crate::diagnostic::{
    Diagnostic, DiagnosticCode, DiagnosticSet, IdlSyntaxError, LoadError, SourceLocation,
};
use crate::json_loader::JsonLoader;
use lexer::Cursor;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use syntax::{
    Annotations, ApplySyntax, IdlDocument, IdlParseError, LineIndex, MemberNameSyntax,
    MemberSyntax, MetadataSyntax, ParseErrorKind, PropertySyntax, PropertyValueSyntax,
    ShapeBodySyntax, ShapeKindSyntax, ShapeSyntax, Span, Spanned, StatementSyntax, TraitSyntax,
    TraitValueSyntax,
};

/// Parses one IDL input under `limits`.
pub(crate) fn load_slice(
    limits: &JsonLoader,
    source: &str,
    input: &[u8],
) -> Result<IdlDocument, LoadError> {
    let source: Arc<str> = Arc::from(source);
    if input.len() as u64 > limits.max_input_bytes {
        return Err(limits.input_too_large(&source));
    }
    let text = match std::str::from_utf8(input) {
        Ok(text) => text,
        Err(err) => {
            let valid = std::str::from_utf8(&input[..err.valid_up_to()])
                .expect("the prefix before the first invalid byte is valid UTF-8");
            let error = IdlParseError::syntax(valid.len(), "input is not valid UTF-8");
            return Err(syntax_error(&source, valid, error));
        }
    };
    parse(
        source.clone(),
        Arc::from(text),
        limits.max_depth,
        limits.max_shapes,
    )
    .map_err(|error| syntax_error(&source, text, error))
}

/// Reads and parses an IDL file, buffering at most one byte beyond the input limit.
pub(crate) fn load_file(limits: &JsonLoader, path: &Path) -> Result<IdlDocument, LoadError> {
    let source = path.display().to_string();
    let file = std::fs::File::open(path).map_err(|err| JsonLoader::open_error(&source, err))?;
    let mut bytes = Vec::new();
    file.take(limits.max_input_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|err| JsonLoader::read_error(&source, err))?;
    load_slice(limits, &source, &bytes)
}

/// Translates a syntax failure into the one diagnostic and error kind for the input.
fn syntax_error(source: &Arc<str>, text: &str, error: IdlParseError) -> LoadError {
    let position = LineIndex::new(text).position(text, error.offset);
    let location = SourceLocation::at_position(source.clone(), position);
    let code = match error.kind {
        ParseErrorKind::Syntax => DiagnosticCode::IdlSyntax,
        ParseErrorKind::DuplicateKey => DiagnosticCode::DuplicateKey,
        ParseErrorKind::ResourceLimit => DiagnosticCode::ResourceLimit,
    };
    let mut diagnostics = DiagnosticSet::default();
    diagnostics.push_unbounded(Diagnostic::error_at(
        code,
        error.message.clone(),
        Some(location),
    ));
    match error.kind {
        ParseErrorKind::ResourceLimit => LoadError::resource_limit(diagnostics),
        _ => LoadError::idl(IdlSyntaxError::new(error.message), diagnostics),
    }
}

pub(crate) fn parse(
    source: Arc<str>,
    text: Arc<str>,
    max_depth: usize,
    max_shapes: usize,
) -> Result<IdlDocument, IdlParseError> {
    Parser::new(&text, max_depth, max_shapes).document(source, text.clone())
}

struct Parser<'a> {
    cursor: Cursor<'a>,
    controls: HashSet<String>,
    metadata: HashSet<String>,
    imports: HashSet<String>,
    declarations: HashSet<String>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, max_depth: usize, max_shapes: usize) -> Self {
        Self {
            cursor: Cursor::new(input, max_depth, max_shapes),
            controls: HashSet::new(),
            metadata: HashSet::new(),
            imports: HashSet::new(),
            declarations: HashSet::new(),
        }
    }

    fn document(mut self, source: Arc<str>, text: Arc<str>) -> Result<IdlDocument, IdlParseError> {
        self.cursor.ws0();
        let mut controls = Vec::new();
        while self.cursor.starts_with("$") {
            controls.push(self.control()?);
            self.cursor.declaration_break()?;
            self.section_trivia();
        }

        let mut metadata = Vec::new();
        while self.peek_keyword("metadata") {
            metadata.push(self.metadata()?);
            self.cursor.declaration_break()?;
            self.section_trivia();
        }

        let namespace = if self.cursor.keyword("namespace") {
            self.cursor.sp1()?;
            let namespace = self.namespace()?;
            self.cursor.declaration_break()?;
            self.section_trivia();
            Some(namespace)
        } else {
            None
        };

        let mut uses = Vec::new();
        while self.peek_keyword("use") {
            if namespace.is_none() {
                return Err(self.cursor.error("a `use` statement requires a namespace"));
            }
            self.cursor.keyword("use");
            self.cursor.sp1()?;
            let id = self.cursor.shape_id()?;
            if !id.absolute || id.member.is_some() {
                return Err(IdlParseError::syntax(
                    id.span.start,
                    "a `use` target must be an absolute root shape ID",
                ));
            }
            let local = id.root_name().to_owned();
            if !self.imports.insert(local.clone()) {
                return Err(IdlParseError::duplicate(id.span.start, &local));
            }
            uses.push(id);
            self.cursor.declaration_break()?;
            self.section_trivia();
        }

        let mut statements = Vec::new();
        while !self.cursor.eof() {
            let annotations = self.annotations()?;
            if self.cursor.eof() {
                if !annotations.traits.is_empty() {
                    return Err(self
                        .cursor
                        .error("a trait must precede a shape or `apply` statement"));
                }
                break;
            }
            if annotations.traits.is_empty() && annotations.documentation.is_none() {
                self.misplaced_section(namespace.is_some())?;
            }
            if namespace.is_none() {
                return Err(self
                    .cursor
                    .error("shape and `apply` statements require a namespace"));
            }
            if self.peek_keyword("apply") {
                statements.push(StatementSyntax::Apply(self.apply(annotations)?));
            } else {
                statements.push(StatementSyntax::Shape(self.shape(annotations)?));
            }
            self.cursor.declaration_break()?;
            self.section_trivia();
        }

        Ok(IdlDocument {
            source,
            lines: syntax::LineIndex::new(&text),
            text,
            controls,
            metadata,
            namespace,
            uses,
            statements,
        })
    }

    /// Reports a section header found where only shape statements may appear. IDL sections
    /// must appear in the order control, metadata, namespace, then shapes.
    fn misplaced_section(&mut self, has_namespace: bool) -> Result<(), IdlParseError> {
        let message = if self.cursor.starts_with("$") {
            "control statements must come before metadata, `namespace`, and shapes"
        } else if self.peek_keyword("metadata") {
            "metadata statements must come after control statements and before `namespace`"
        } else if self.peek_keyword("namespace") {
            if has_namespace {
                "a model file can contain only one `namespace` statement"
            } else {
                "`namespace` must come after control and metadata statements"
            }
        } else if self.peek_keyword("use") {
            "`use` statements must come after `namespace` and before shapes"
        } else {
            return Ok(());
        };
        Err(self.cursor.error(message))
    }

    fn section_trivia(&mut self) {
        let checkpoint = self.cursor.pos;
        let _ = self.cursor.documentation();
        let is_section_header = self.cursor.starts_with("$")
            || self.peek_keyword("metadata")
            || self.peek_keyword("namespace")
            || self.peek_keyword("use");
        if !is_section_header {
            self.cursor.pos = checkpoint;
        }
    }

    fn peek_keyword(&mut self, value: &str) -> bool {
        let checkpoint = self.cursor.pos;
        let matched = self.cursor.keyword(value);
        self.cursor.pos = checkpoint;
        matched
    }

    fn control(&mut self) -> Result<(Spanned<String>, syntax::NodeSyntax), IdlParseError> {
        self.cursor.expect("$", "a control statement")?;
        let key = self.cursor.object_key()?;
        if !self.controls.insert(key.value.clone()) {
            return Err(IdlParseError::duplicate(key.span.start, &key.value));
        }
        self.cursor.ws0();
        self.cursor.expect(":", "`:` after a control name")?;
        let value = self.same_line_value()?;
        Ok((key, value))
    }

    fn metadata(&mut self) -> Result<MetadataSyntax, IdlParseError> {
        let start = self.cursor.pos;
        self.cursor.keyword("metadata");
        self.cursor.sp1()?;
        let key = self.cursor.object_key()?;
        if !self.metadata.insert(key.value.clone()) {
            return Err(IdlParseError::duplicate(key.span.start, &key.value));
        }
        self.cursor.ws0();
        self.cursor.expect("=", "`=` after a metadata key")?;
        let value = self.same_line_value()?;
        Ok(MetadataSyntax {
            key,
            value,
            span: Span::new(start, self.cursor.pos),
        })
    }

    fn namespace(&mut self) -> Result<Spanned<String>, IdlParseError> {
        let start = self.cursor.pos;
        let mut value = self.cursor.identifier()?.value;
        while self.cursor.consume(".") {
            value.push('.');
            value.push_str(&self.cursor.identifier()?.value);
        }
        Ok(Spanned::new(value, Span::new(start, self.cursor.pos)))
    }

    fn annotations(&mut self) -> Result<Annotations, IdlParseError> {
        let documentation = self.cursor.documentation();
        let mut traits = Vec::new();
        loop {
            if !self.cursor.consume("@") {
                break;
            }
            let start = self.cursor.pos - 1;
            let id = self.cursor.shape_id()?;
            self.cursor.sp0();
            let at = Span::new(start, start + 1);
            let value = if self.cursor.starts_with("(") {
                let open = self.cursor.expect("(", "`(`")?;
                match self.cursor.trait_value(open, 0)? {
                    TraitValueSyntax::Omitted { .. } => TraitValueSyntax::Omitted { at },
                    explicit => explicit,
                }
            } else {
                TraitValueSyntax::Omitted { at }
            };
            traits.push(TraitSyntax {
                id,
                value,
                span: Span::new(start, self.cursor.pos),
            });
            // Documentation is only applied when it precedes every trait. A late `///`
            // block is ordinary trivia, matching Smithy Java.
            let _ = self.cursor.documentation();
        }
        Ok(Annotations {
            documentation,
            traits,
        })
    }

    fn apply(&mut self, leading: Annotations) -> Result<ApplySyntax, IdlParseError> {
        let start = self.cursor.pos;
        self.cursor.keyword("apply");
        self.cursor.sp1()?;
        let target = self.cursor.shape_id()?;
        let mut trailing = self.annotations()?;
        let singular_end = trailing.traits.last().map(|value| value.span.end);
        if self.cursor.consume("{") {
            let block = self.annotations()?;
            trailing.documentation = trailing.documentation.or(block.documentation);
            trailing.traits.extend(block.traits);
            if !self.cursor.consume("}") {
                return Err(self.cursor.error(if self.cursor.eof() {
                    "expected `}` after applied traits"
                } else {
                    "expected a trait application or `}` in `apply`"
                }));
            }
        } else if let Some(end) = singular_end {
            self.cursor.pos = end;
        }
        let mut traits = leading.traits;
        traits.extend(trailing.traits);
        if traits.is_empty() {
            return Err(self
                .cursor
                .error("an `apply` statement requires at least one trait"));
        }
        Ok(ApplySyntax {
            annotations: Annotations {
                documentation: leading.documentation.or(trailing.documentation),
                traits,
            },
            target,
            span: Span::new(start, self.cursor.pos),
        })
    }

    fn shape(&mut self, annotations: Annotations) -> Result<ShapeSyntax, IdlParseError> {
        let start = self.cursor.pos;
        let kind_token = self.cursor.identifier()?;
        let kind = shape_kind(&kind_token.value).ok_or_else(|| {
            IdlParseError::syntax(
                kind_token.span.start,
                format!("unknown shape type `{}`", kind_token.value),
            )
        })?;
        self.cursor.sp1()?;
        let name = self.cursor.identifier()?;
        if self.imports.contains(&name.value) {
            return Err(IdlParseError {
                offset: name.span.start,
                kind: syntax::ParseErrorKind::DuplicateKey,
                message: format!("`{}` conflicts with an imported shape name", name.value),
            });
        }
        if !self.declarations.insert(name.value.clone()) {
            return Err(IdlParseError::duplicate(name.span.start, &name.value));
        }
        self.count_shape(name.span.start)?;

        self.cursor.sp0();
        let for_start = self.cursor.pos;
        let resource = if self.cursor.keyword("for") {
            // ABNF `AggregateShape` permits `ForResource` on list, map, union, and structure.
            if !matches!(
                kind,
                ShapeKindSyntax::List
                    | ShapeKindSyntax::Map
                    | ShapeKindSyntax::Union
                    | ShapeKindSyntax::Structure
                    | ShapeKindSyntax::Set
            ) {
                return Err(IdlParseError::syntax(
                    for_start,
                    "`for` is only valid on list, map, union, and structure shapes",
                ));
            }
            self.cursor.sp1()?;
            let id = self.cursor.shape_id()?;
            self.cursor.sp0();
            Some(id)
        } else {
            None
        };
        let mixins = self.mixins()?;

        let body = if kind.is_simple() {
            ShapeBodySyntax::None
        } else if matches!(
            kind,
            ShapeKindSyntax::List
                | ShapeKindSyntax::Map
                | ShapeKindSyntax::Structure
                | ShapeKindSyntax::Union
                | ShapeKindSyntax::Enum
                | ShapeKindSyntax::IntEnum
                | ShapeKindSyntax::Set
        ) {
            ShapeBodySyntax::Members(self.members(kind)?)
        } else {
            ShapeBodySyntax::Properties(self.properties(kind)?)
        };

        Ok(ShapeSyntax {
            annotations,
            kind,
            name,
            resource,
            mixins,
            body,
            span: Span::new(start, self.cursor.pos),
        })
    }

    /// Parses an optional `= value`, which must end its line: ABNF `ValueAssignment` is
    /// `[SP] "=" [SP] NodeValue [SP] [Comma] BR`.
    fn value_assignment(&mut self) -> Result<Option<syntax::NodeSyntax>, IdlParseError> {
        if !self.cursor.consume("=") {
            return Ok(None);
        }
        let value = self.same_line_value()?;
        self.cursor.sp0();
        self.cursor.consume(",");
        self.cursor.declaration_break()?;
        Ok(Some(value))
    }

    /// A node value after `:` or `=` in a control, metadata, or value assignment statement.
    /// The grammar allows only spaces before it (`[SP] NodeValue`), not a line break.
    fn same_line_value(&mut self) -> Result<syntax::NodeSyntax, IdlParseError> {
        self.cursor.sp0();
        if self.cursor.eof()
            || self.cursor.starts_with("\n")
            || self.cursor.starts_with("\r")
            || self.cursor.starts_with("//")
        {
            return Err(self.cursor.error("expected a value on the same line"));
        }
        self.cursor.node(0)
    }

    fn mixins(&mut self) -> Result<Vec<syntax::ShapeIdSyntax>, IdlParseError> {
        if !self.cursor.keyword("with") {
            return Ok(Vec::new());
        }
        self.cursor.ws0();
        self.cursor.expect("[", "`[` after `with`")?;
        self.cursor.ws0();
        if self.cursor.starts_with("]") {
            return Err(self
                .cursor
                .error("a `with` clause requires at least one mixin"));
        }
        let mut values = Vec::new();
        loop {
            values.push(self.cursor.shape_id()?);
            self.cursor.ws0();
            if self.cursor.consume("]") {
                break;
            }
        }
        Ok(values)
    }

    fn count_shape(&mut self, offset: usize) -> Result<(), IdlParseError> {
        self.cursor.shapes += 1;
        if self.cursor.shapes > self.cursor.max_shapes {
            Err(IdlParseError::resource(
                offset,
                format!(
                    "document exceeds the maximum of {} shapes",
                    self.cursor.max_shapes
                ),
            ))
        } else {
            Ok(())
        }
    }

    fn members(&mut self, kind: ShapeKindSyntax) -> Result<Vec<MemberSyntax>, IdlParseError> {
        self.cursor.ws0();
        self.cursor.expect("{", "`{` before shape members")?;
        let mut members = Vec::new();
        let mut names = HashSet::new();
        loop {
            let annotations = self.annotations()?;
            if self.cursor.starts_with("}") {
                let is_enum = matches!(kind, ShapeKindSyntax::Enum | ShapeKindSyntax::IntEnum);
                // A trailing documentation comment is ignored, as in Smithy Java, except in
                // enums, where Java expects a member after it. A trait always needs a member.
                if !annotations.traits.is_empty()
                    || (is_enum && annotations.documentation.is_some())
                {
                    return Err(self
                        .cursor
                        .error("annotations must precede a member declaration"));
                }
                if is_enum && members.is_empty() {
                    return Err(self.cursor.error("an enum requires at least one member"));
                }
                self.cursor.consume("}");
                break;
            }
            if self.cursor.eof() {
                return Err(self.cursor.error("expected `}` after shape members"));
            }
            let start = self.cursor.pos;
            let name = if self.cursor.consume("$") {
                let value = self.cursor.identifier()?;
                MemberNameSyntax::Elided(Spanned::new(
                    value.value,
                    Span::new(start, value.span.end),
                ))
            } else {
                MemberNameSyntax::Named(self.cursor.identifier()?)
            };
            if matches!(kind, ShapeKindSyntax::Enum | ShapeKindSyntax::IntEnum)
                && matches!(name, MemberNameSyntax::Elided(_))
            {
                return Err(self.cursor.error("enum members cannot use target elision"));
            }
            if !names.insert(name.value().to_owned()) {
                return Err(IdlParseError::duplicate(name.span().start, name.value()));
            }
            self.cursor.sp0();

            let (target, value) =
                if matches!(kind, ShapeKindSyntax::Enum | ShapeKindSyntax::IntEnum)
                    || matches!(name, MemberNameSyntax::Elided(_))
                {
                    let value = self.value_assignment()?;
                    (None, value)
                } else {
                    self.cursor.expect(":", "`:` after a member name")?;
                    self.cursor.sp0();
                    let target = self.cursor.shape_id()?;
                    self.cursor.sp0();
                    let value = self.value_assignment()?;
                    (Some(target), value)
                };
            members.push(MemberSyntax {
                annotations,
                name,
                target,
                value,
                span: Span::new(start, self.cursor.pos),
            });
        }
        Ok(members)
    }

    fn properties(&mut self, kind: ShapeKindSyntax) -> Result<Vec<PropertySyntax>, IdlParseError> {
        self.cursor.ws0();
        self.cursor.expect("{", "`{` before shape properties")?;
        let mut properties = Vec::new();
        let mut names = HashSet::new();
        loop {
            self.cursor.ws0();
            if self.cursor.consume("}") {
                break;
            }
            if self.cursor.eof() {
                return Err(self.cursor.error("expected `}` after shape properties"));
            }
            let start = self.cursor.pos;
            let name = if kind == ShapeKindSyntax::Operation {
                self.cursor.identifier()?
            } else {
                self.cursor.object_key()?
            };
            if !names.insert(name.value.clone()) {
                return Err(IdlParseError::duplicate(name.span.start, &name.value));
            }
            self.cursor.ws0();
            let assign = self.cursor.pos;
            let value = if kind == ShapeKindSyntax::Operation && self.cursor.consume(":=") {
                // Only `input` and `output` generate a shape; lowering rejects any other.
                if matches!(name.value.as_str(), "input" | "output") {
                    self.count_shape(name.span.start)?;
                }
                self.cursor.ws0();
                let annotations = self.annotations()?;
                let resource = if self.cursor.keyword("for") {
                    self.cursor.sp1()?;
                    let resource = self.cursor.shape_id()?;
                    self.cursor.ws0();
                    Some(resource)
                } else {
                    None
                };
                let mixins = self.mixins()?;
                let members = self.members(ShapeKindSyntax::Structure)?;
                PropertyValueSyntax::InlineStructure {
                    annotations,
                    members,
                    resource,
                    mixins,
                    span: Span::new(assign, self.cursor.pos),
                }
            } else {
                self.cursor.expect(":", "`:` after a property name")?;
                PropertyValueSyntax::Node(self.cursor.node(0)?)
            };
            properties.push(PropertySyntax {
                name,
                value,
                span: Span::new(start, self.cursor.pos),
            });
        }
        Ok(properties)
    }
}

fn shape_kind(value: &str) -> Option<ShapeKindSyntax> {
    Some(match value {
        "blob" => ShapeKindSyntax::Blob,
        "boolean" => ShapeKindSyntax::Boolean,
        "document" => ShapeKindSyntax::Document,
        "string" => ShapeKindSyntax::String,
        "byte" => ShapeKindSyntax::Byte,
        "short" => ShapeKindSyntax::Short,
        "integer" => ShapeKindSyntax::Integer,
        "long" => ShapeKindSyntax::Long,
        "float" => ShapeKindSyntax::Float,
        "double" => ShapeKindSyntax::Double,
        "bigInteger" => ShapeKindSyntax::BigInteger,
        "bigDecimal" => ShapeKindSyntax::BigDecimal,
        "timestamp" => ShapeKindSyntax::Timestamp,
        "list" => ShapeKindSyntax::List,
        "map" => ShapeKindSyntax::Map,
        "structure" => ShapeKindSyntax::Structure,
        "union" => ShapeKindSyntax::Union,
        "enum" => ShapeKindSyntax::Enum,
        "intEnum" => ShapeKindSyntax::IntEnum,
        "service" => ShapeKindSyntax::Service,
        "resource" => ShapeKindSyntax::Resource,
        "operation" => ShapeKindSyntax::Operation,
        "set" => ShapeKindSyntax::Set,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::{NodeSyntax, ParseErrorKind};

    fn parse_model(input: &str) -> Result<IdlDocument, IdlParseError> {
        parse(Arc::from("test.smithy"), Arc::from(input), 16, 100)
    }

    #[test]
    fn parses_sections_and_every_shape_kind() {
        let input = r#"$version: "2"
metadata key = required
namespace example
use other.ns#Imported

blob Blob
boolean Boolean
string String
list List { member: String }
map Map { key: String, value: Blob }
structure Struct for Resource { /// docs
    @required
    member: Imported = "x"
    $id
}
union Union { value: String }
enum Enum { A, B = "bee"
}
intEnum IntEnum { A = 1
}
service Service { version: "1", operations: [Operation] }
resource Resource { identifiers: {id: String}, read: Operation }
operation Operation { input := { id: String }, output: Struct, errors: [] }
"#;
        let document = parse_model(input).unwrap();
        assert_eq!(document.namespace.as_ref().unwrap().value, "example");
        assert_eq!(document.controls.len(), 1);
        assert_eq!(document.metadata.len(), 1);
        assert_eq!(document.uses.len(), 1);
        assert_eq!(document.statements.len(), 12);
    }

    #[test]
    fn retains_docs_traits_defaults_and_inline_io() {
        let input = r#"namespace ex
/// shape docs
@trait(foo: "bar")
structure Input {
    /// member docs
    @required
    name: String = "x"
}
operation Op { input := @input { value: String } }
"#;
        let document = parse_model(input).unwrap();
        let StatementSyntax::Shape(shape) = &document.statements[0] else {
            panic!()
        };
        assert_eq!(
            shape.annotations.documentation.as_ref().unwrap().value,
            "shape docs"
        );
        assert_eq!(shape.annotations.traits.len(), 1);
        let ShapeBodySyntax::Members(members) = &shape.body else {
            panic!()
        };
        assert!(members[0].value.is_some());
        assert_eq!(
            members[0].annotations.documentation.as_ref().unwrap().value,
            "member docs"
        );
    }

    #[test]
    fn syntactic_ids_are_not_quoted_strings() {
        let document = parse_model("metadata a = required\nnamespace ex\nstring S\n").unwrap();
        assert!(matches!(
            document.metadata[0].value,
            NodeSyntax::SyntacticShapeId(_)
        ));
    }

    #[test]
    fn recognizes_unsupported_constructs_for_later_diagnostics() {
        let document = parse_model(
            "namespace ex\nset Old { member: String }\nstructure S with [M] {}\napply S @required\n",
        )
        .unwrap();
        let StatementSyntax::Shape(shape) = &document.statements[0] else {
            panic!()
        };
        assert_eq!(shape.kind, ShapeKindSyntax::Set);
        let StatementSyntax::Shape(shape) = &document.statements[1] else {
            panic!()
        };
        assert_eq!(shape.mixins.len(), 1);
        assert!(matches!(document.statements[2], StatementSyntax::Apply(_)));
    }

    #[test]
    fn rejects_duplicates_invalid_sections_and_trailing_annotations() {
        for input in [
            "namespace ex\nstructure S { a: String, a: String }",
            "namespace ex\nuse a#S\nuse b#S\n",
            "namespace ex\nstring S\nstring S\n",
            "namespace ex\nservice S { version: \"1\", version: \"2\" }",
            "namespace ex\n@required",
            "string MissingNamespace",
            "namespace ex structure SameLine",
        ] {
            assert!(parse_model(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn limits_explicit_and_inline_shapes() {
        let error = parse(
            Arc::from("test"),
            Arc::from("namespace ex\noperation O { input := {} }"),
            8,
            1,
        )
        .unwrap_err();
        assert_eq!(error.kind, ParseErrorKind::ResourceLimit);
    }

    #[test]
    fn unicode_columns_use_characters() {
        let input = "metadata x = \"é\"\nnamespace ex\nstring S";
        let document = parse_model(input).unwrap();
        let position = document.position(document.statements[0].clone().span());
        assert_eq!(position.line, 3);
        assert_eq!(position.column, 1);
    }

    #[test]
    fn optional_shape_section_documents_are_valid() {
        for input in [
            "",
            "// comment only",
            "$version: \"2.0\"",
            "metadata \"key\" = \"value\"",
        ] {
            let document =
                parse_model(input).unwrap_or_else(|error| panic!("{input:?}: {error:?}"));
            assert!(document.namespace.is_none());
            assert!(document.statements.is_empty());
        }
    }

    #[test]
    fn apply_forms_and_inline_mixins_are_retained() {
        let document = parse_model(
            "namespace ex\napply S @required\napply S { @required @sensitive }\noperation O { input := with [Base] {} }\n",
        )
        .unwrap();
        assert!(matches!(document.statements[0], StatementSyntax::Apply(_)));
        assert!(matches!(document.statements[1], StatementSyntax::Apply(_)));
        let StatementSyntax::Shape(operation) = &document.statements[2] else {
            panic!()
        };
        let ShapeBodySyntax::Properties(properties) = &operation.body else {
            panic!()
        };
        let PropertyValueSyntax::InlineStructure { mixins, .. } = &properties[0].value else {
            panic!()
        };
        assert_eq!(mixins.len(), 1);
    }

    #[test]
    fn later_member_docs_and_section_header_docs_are_classified() {
        let document = parse_model(
            "$version: \"2\"\n/// section trivia\nnamespace ex\n/// section trivia\nuse other#Thing\nstructure S {\n first: String\n /// second docs\n second: Thing\n}\n",
        )
        .unwrap();
        let StatementSyntax::Shape(shape) = &document.statements[0] else {
            panic!()
        };
        assert!(shape.annotations.documentation.is_none());
        let ShapeBodySyntax::Members(members) = &shape.body else {
            panic!()
        };
        assert_eq!(
            members[1].annotations.documentation.as_ref().unwrap().value,
            "second docs"
        );
    }

    #[test]
    fn malformed_text_block_errors_stay_on_character_boundaries() {
        let input = "metadata key = \"\"\"\n \\é\n \"\"\"";
        let error = parse_model(input).unwrap_err();
        assert!(input.is_char_boundary(error.offset));
        let position = syntax::LineIndex::new(input).position(input, error.offset);
        assert_eq!(position.line, 1);
    }

    trait StatementSpan {
        fn span(&self) -> Span;
    }

    impl StatementSpan for StatementSyntax {
        fn span(&self) -> Span {
            match self {
                StatementSyntax::Shape(value) => value.span,
                StatementSyntax::Apply(value) => value.span,
            }
        }
    }
}
