/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Errors from building a dynamic client.

use aws_smithy_lang::runtime_schema::SchemaBuildError;
use aws_smithy_lang::{LoadError, ShapeId};
use aws_smithy_runtime_api::box_error::BoxError;
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
    ModelLoadFailed {
        path: PathBuf,
        source: LoadError,
    },
    NoService,
    AmbiguousService {
        services: Vec<ShapeId>,
    },
    UnknownService {
        id: ShapeId,
        services: Vec<ShapeId>,
    },
    SchemaBuildFailed {
        source: SchemaBuildError,
    },
    ProtocolNotFound {
        service: ShapeId,
        traits: Vec<ShapeId>,
    },
    MissingBehaviorVersion,
    MissingServiceVersion {
        service: ShapeId,
    },
    InvalidConfig {
        source: BoxError,
    },
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

    pub(crate) fn protocol_not_found(service: ShapeId, traits: Vec<ShapeId>) -> Self {
        Self {
            kind: BuildErrorKind::ProtocolNotFound { service, traits },
        }
    }

    pub(crate) fn missing_behavior_version() -> Self {
        Self {
            kind: BuildErrorKind::MissingBehaviorVersion,
        }
    }

    pub(crate) fn missing_service_version(service: ShapeId) -> Self {
        Self {
            kind: BuildErrorKind::MissingServiceVersion { service },
        }
    }

    pub(crate) fn invalid_config(source: BoxError) -> Self {
        Self {
            kind: BuildErrorKind::InvalidConfig { source },
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
            BuildErrorKind::ProtocolNotFound { service, traits } if traits.is_empty() => {
                write!(
                    f,
                    "service `{service}` has no protocol trait; set a protocol with `protocol(..)`"
                )
            }
            BuildErrorKind::ProtocolNotFound { service, traits } => {
                write!(
                    f,
                    "no protocol implementation matches the protocol traits of service \
                     `{service}`: "
                )?;
                write_ids(f, traits)?;
                write!(f, "; set a protocol with `protocol(..)`")
            }
            BuildErrorKind::MissingBehaviorVersion => write!(
                f,
                "a behavior version is required: set one with \
                 `Config::builder().behavior_version(..)`"
            ),
            BuildErrorKind::MissingServiceVersion { service } => write!(
                f,
                "service `{service}` uses the awsQuery protocol, which needs a service \
                 version, but the service has no `version`"
            ),
            BuildErrorKind::InvalidConfig { .. } => write!(f, "invalid client configuration"),
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
            BuildErrorKind::ProtocolNotFound { .. } => None,
            BuildErrorKind::MissingBehaviorVersion => None,
            BuildErrorKind::MissingServiceVersion { .. } => None,
            BuildErrorKind::InvalidConfig { source } => Some(source.as_ref()),
        }
    }
}

// A config validator error: the config has no HTTP client, and no default one is available.
#[derive(Debug)]
pub(crate) struct MissingHttpClient;

impl fmt::Display for MissingHttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "an HTTP client is required: set one with `Config::builder().http_client(..)`, or \
             enable the `default-https-client` feature"
        )
    }
}

impl Error for MissingHttpClient {}

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
