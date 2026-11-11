/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors from building a dynamic client.

use aws_smithy_lang::runtime_schema::SchemaBuildError;
use aws_smithy_lang::{LoadError, ShapeId};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

/// An error from [`Builder::build`](crate::client::Builder::build).
#[derive(Debug)]
pub struct BuildError {
    kind: BuildErrorKind,
}

#[derive(Debug)]
pub(crate) enum BuildErrorKind {
    MissingModel,
    ModelLoadFailed { path: PathBuf, source: LoadError },
    NoService,
    AmbiguousService { services: Vec<ShapeId> },
    UnknownService { id: ShapeId, services: Vec<ShapeId> },
    SchemaBuildFailed { source: SchemaBuildError },
}

impl BuildError {
    pub(crate) fn missing_model() -> Self {
        Self {
            kind: BuildErrorKind::MissingModel,
        }
    }

    pub(crate) fn model_load_failed(path: PathBuf, source: LoadError) -> Self {
        Self {
            kind: BuildErrorKind::ModelLoadFailed { path, source },
        }
    }

    pub(crate) fn no_service() -> Self {
        Self {
            kind: BuildErrorKind::NoService,
        }
    }

    pub(crate) fn ambiguous_service(services: Vec<ShapeId>) -> Self {
        Self {
            kind: BuildErrorKind::AmbiguousService { services },
        }
    }

    pub(crate) fn unknown_service(id: ShapeId, services: Vec<ShapeId>) -> Self {
        Self {
            kind: BuildErrorKind::UnknownService { id, services },
        }
    }

    pub(crate) fn schema_build_failed(source: SchemaBuildError) -> Self {
        Self {
            kind: BuildErrorKind::SchemaBuildFailed { source },
        }
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> &BuildErrorKind {
        &self.kind
    }
}

// Writes `a#A`, `b#B`.
fn write_ids(f: &mut fmt::Formatter<'_>, ids: &[ShapeId]) -> fmt::Result {
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "`{id}`")?;
    }
    Ok(())
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            BuildErrorKind::MissingModel => {
                write!(f, "a model is required: set one with `model_file(..)`")
            }
            BuildErrorKind::ModelLoadFailed { path, .. } => {
                write!(f, "failed to load the model from `{}`", path.display())
            }
            BuildErrorKind::NoService => write!(f, "the model has no service shape"),
            BuildErrorKind::AmbiguousService { services } => {
                write!(f, "the model has several services: ")?;
                write_ids(f, services)?;
                write!(f, "; pick one with `service(..)`")
            }
            BuildErrorKind::UnknownService { id, services } => {
                write!(f, "the model has no service `{id}`")?;
                if !services.is_empty() {
                    write!(f, "; its services are ")?;
                    write_ids(f, services)?;
                }
                Ok(())
            }
            BuildErrorKind::SchemaBuildFailed { .. } => {
                write!(f, "failed to build schemas for the service")
            }
        }
    }
}

impl Error for BuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            BuildErrorKind::MissingModel => None,
            BuildErrorKind::ModelLoadFailed { source, .. } => Some(source),
            BuildErrorKind::NoService => None,
            BuildErrorKind::AmbiguousService { .. } => None,
            BuildErrorKind::UnknownService { .. } => None,
            BuildErrorKind::SchemaBuildFailed { source } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BuildError;
    use aws_smithy_lang::Model;
    use std::error::Error;
    use std::path::PathBuf;

    #[test]
    fn model_load_failed_keeps_the_load_error_as_its_source() {
        let source = Model::from_json_str("bad.json", "{").unwrap_err();
        let err = BuildError::model_load_failed(PathBuf::from("bad.json"), source);
        assert!(err.source().is_some());
    }
}
