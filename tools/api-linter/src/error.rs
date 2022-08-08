/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::{Context, Result};
use owo_colors::{OwoColorize, Stream};
use pest::Position;
use rustdoc_types::Span;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Where the error occurred relative to the [`Path`](crate::path::Path).
///
/// For example, if the path is a path to a function, then this could point to something
/// specific about that function, such as a specific function argument that is in error.
///
/// There is overlap in this enum with [`ComponentType`](crate::path::ComponentType) since
/// some paths are specific enough to locate the external type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ErrorLocation {
    AssocType,
    ArgumentNamed(String),
    ClosureInput,
    ClosureOutput,
    ConstGeneric,
    Constant,
    EnumTupleEntry,
    GenericArg,
    GenericDefaultBinding,
    ImplementedTrait,
    QualifiedSelfType,
    QualifiedSelfTypeAsTrait,
    ReExport,
    ReturnValue,
    Static,
    StructField,
    TraitBound,
    TypeDef,
    WhereBound,
}

impl fmt::Display for ErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::AssocType => "associated type",
            Self::ArgumentNamed(name) => return write!(f, "argument named `{}` of", name),
            Self::ClosureInput => "closure input of",
            Self::ClosureOutput => "closure output of",
            Self::ConstGeneric => "const generic of",
            Self::Constant => "constant",
            Self::EnumTupleEntry => "enum tuple entry of",
            Self::GenericArg => "generic arg of",
            Self::GenericDefaultBinding => "generic default binding of",
            Self::ImplementedTrait => "implemented trait of",
            Self::QualifiedSelfType => "qualified self type",
            Self::QualifiedSelfTypeAsTrait => "qualified type `as` trait",
            Self::ReExport => "re-export named",
            Self::ReturnValue => "return value of",
            Self::Static => "static value",
            Self::StructField => "struct field of",
            Self::TraitBound => "trait bound of",
            Self::TypeDef => "typedef type of",
            Self::WhereBound => "where bound of",
        };
        write!(f, "{}", s)
    }
}

/// Error type for validation errors that get displayed to the user on the CLI.
#[derive(Debug)]
pub enum ValidationError {
    UnapprovedExternalTypeRef {
        type_name: String,
        what: ErrorLocation,
        in_what_type: String,
        location: Option<Span>,
        sort_key: String,
    },
}

impl ValidationError {
    pub fn unapproved_external_type_ref(
        type_name: impl Into<String>,
        what: &ErrorLocation,
        in_what_type: impl Into<String>,
        location: Option<&Span>,
    ) -> Self {
        let type_name = type_name.into();
        let in_what_type = in_what_type.into();
        let sort_key = format!(
            "{}:{}:{}:{}",
            location_sort_key(location),
            type_name,
            what,
            in_what_type
        );
        Self::UnapprovedExternalTypeRef {
            type_name,
            what: what.clone(),
            in_what_type,
            location: location.cloned(),
            sort_key,
        }
    }

    pub fn type_name(&self) -> &str {
        match self {
            Self::UnapprovedExternalTypeRef { type_name, .. } => type_name,
        }
    }

    pub fn location(&self) -> Option<&Span> {
        match self {
            Self::UnapprovedExternalTypeRef { location, .. } => location.as_ref(),
        }
    }

    fn sort_key(&self) -> &str {
        match self {
            Self::UnapprovedExternalTypeRef { sort_key, .. } => sort_key.as_ref(),
        }
    }

    pub fn fmt_headline(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnapprovedExternalTypeRef { type_name, .. } => {
                let inner = format!(
                    "Unapproved external type `{}` referenced in public API",
                    type_name
                );
                write!(
                    f,
                    "{} {}",
                    "error:"
                        .if_supports_color(Stream::Stdout, |text| text.red())
                        .if_supports_color(Stream::Stdout, |text| text.bold()),
                    inner.if_supports_color(Stream::Stdout, |text| text.bold())
                )
            }
        }
    }

    pub fn subtext(&self) -> String {
        match self {
            Self::UnapprovedExternalTypeRef {
                what, in_what_type, ..
            } => format!("in {} `{}`", what, in_what_type),
        }
    }
}

fn location_sort_key(location: Option<&Span>) -> String {
    if let Some(location) = location {
        format!(
            "{}:{:07}:{:07}",
            location.filename.to_string_lossy(),
            location.begin.0,
            location.begin.1
        )
    } else {
        "none".into()
    }
}

impl Ord for ValidationError {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl PartialOrd for ValidationError {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.sort_key().partial_cmp(other.sort_key())
    }
}

impl Eq for ValidationError {}

impl PartialEq for ValidationError {
    fn eq(&self, other: &Self) -> bool {
        self.sort_key() == other.sort_key()
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_headline(f)
    }
}

/// Pretty printer for error context.
///
/// This makes validation errors look similar to the compiler errors from rustc.
pub struct ErrorPrinter {
    workspace_root: PathBuf,
    file_cache: HashMap<PathBuf, String>,
}

impl ErrorPrinter {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            file_cache: HashMap::new(),
        }
    }

    fn get_file_contents(&mut self, path: &Path) -> Result<&str> {
        if !self.file_cache.contains_key(path) {
            let full_file_name = self.workspace_root.join(path).canonicalize()?;
            let contents = std::fs::read_to_string(&full_file_name)
                .context("failed to load source file for error context")
                .context(full_file_name.to_string_lossy().to_string())?;
            self.file_cache.insert(path.to_path_buf(), contents);
        }
        Ok(self.file_cache.get(path).unwrap())
    }

    pub fn pretty_print_error_context(&mut self, location: &Span, subtext: String) {
        match self.get_file_contents(&location.filename) {
            Ok(file_contents) => {
                let begin = Self::position_from_line_col(file_contents, location.begin);
                let end = Self::position_from_line_col(file_contents, location.end);

                // HACK: Using Pest to do the pretty error context formatting for lack of
                // knowledge of a smaller library tailored to this use-case
                let variant = pest::error::ErrorVariant::<()>::CustomError { message: subtext };
                let err_context = match (begin, end) {
                    (Some(b), Some(e)) => {
                        Some(pest::error::Error::new_from_span(variant, b.span(&e)))
                    }
                    (Some(b), None) => Some(pest::error::Error::new_from_pos(variant, b)),
                    _ => None,
                };
                if let Some(err_context) = err_context {
                    println!(
                        "{}\n",
                        err_context.with_path(&location.filename.to_string_lossy())
                    );
                }
            }
            Err(err) => {
                println!("error: {subtext}");
                println!(
                    "  --> {}:{}:{}",
                    location.filename.to_string_lossy(),
                    location.begin.0,
                    location.begin.1 + 1
                );
                println!("   | Failed to load {:?}", location.filename);
                println!("   | relative to {:?}", self.workspace_root);
                println!("   | to provide error message context.");
                println!("   | Cause: {err:?}");
            }
        }
    }

    fn position_from_line_col(contents: &str, (line, col): (usize, usize)) -> Option<Position> {
        let (mut cl, mut cc) = (1, 1);
        let content_bytes = contents.as_bytes();
        for (index, &byte) in content_bytes.iter().enumerate() {
            if cl == line && cc == col {
                return Position::new(contents, index);
            }

            cc += 1;
            if byte == b'\n' {
                cl += 1;
                cc = 0;
            }
        }
        None
    }
}
