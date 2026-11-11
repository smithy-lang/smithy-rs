/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors from building a dynamic client.

use aws_smithy_lang::LoadError;
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

    #[cfg(test)]
    pub(crate) fn kind(&self) -> &BuildErrorKind {
        &self.kind
    }
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
        }
    }
}

impl Error for BuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            BuildErrorKind::MissingModel => None,
            BuildErrorKind::ModelLoadFailed { source, .. } => Some(source),
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
