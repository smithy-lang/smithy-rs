/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The dynamic client and its builder.

use crate::error::BuildError;
use aws_smithy_lang::Model;
use std::path::PathBuf;

/// A client for a Smithy service, built from a model loaded at runtime.
///
/// Create one with [`DynamicClient::builder`].
///
/// ```no_run
/// use aws_smithy_dynamic_client::DynamicClient;
///
/// let client = DynamicClient::builder()
///     .model_file("weather.json")
///     .build()?;
/// # Ok::<(), aws_smithy_dynamic_client::BuildError>(())
/// ```
#[derive(Clone, Debug)]
pub struct DynamicClient {
    // TODO(dynamic-client): Remove this `allow` once service selection reads the model.
    #[allow(dead_code)]
    model: Model,
}

impl DynamicClient {
    /// Returns a builder for a dynamic client.
    pub fn builder() -> Builder {
        Builder::default()
    }
}

/// A builder for a [`DynamicClient`].
#[derive(Clone, Debug, Default)]
pub struct Builder {
    model_file: Option<PathBuf>,
}

impl Builder {
    /// Sets the path of the Smithy JSON AST file to load the model from.
    ///
    /// [`build`](Builder::build) reads the file.
    pub fn model_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.model_file = Some(path.into());
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// Fails when the builder has no model, or when the model file fails to load.
    pub fn build(self) -> Result<DynamicClient, BuildError> {
        let path = self.model_file.ok_or_else(BuildError::missing_model)?;
        let model = Model::from_json_file(&path)
            .map_err(|source| BuildError::model_load_failed(path, source))?;
        Ok(DynamicClient { model })
    }
}

#[cfg(test)]
mod tests {
    use super::DynamicClient;
    use crate::error::BuildErrorKind;
    use std::path::PathBuf;

    fn model(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/models")
            .join(name)
    }

    #[test]
    fn builds_from_a_json_ast_file() {
        DynamicClient::builder()
            .model_file(model("weather.json"))
            .build()
            .unwrap();
    }

    #[test]
    fn fails_without_a_model() {
        let err = DynamicClient::builder().build().unwrap_err();
        assert!(matches!(err.kind(), BuildErrorKind::MissingModel));
    }

    #[test]
    fn fails_on_a_missing_file() {
        let path = model("missing.json");
        let err = DynamicClient::builder()
            .model_file(&path)
            .build()
            .unwrap_err();
        assert!(
            matches!(err.kind(), BuildErrorKind::ModelLoadFailed { path: p, .. } if *p == path)
        );
    }
}
