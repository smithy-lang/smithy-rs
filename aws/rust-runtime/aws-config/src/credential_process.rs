/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Credentials Provider for external process

use crate::json_credentials::{parse_json_credentials, JsonCredentials};
use aws_types::credentials::{future, CredentialsError, ProvideCredentials};
use aws_types::{credentials, Credentials};
use std::process::Command;

/// Credentials Provider
#[derive(Debug)]
pub struct CredentialProcessProvider {
    credential_process: String,
}

impl ProvideCredentials for CredentialProcessProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        future::ProvideCredentials::new(self.credentials())
    }
}

impl CredentialProcessProvider {
    /// Create new [`CredentialProcessProvider`]
    pub fn new(credential_process: String) -> Self {
        Self { credential_process }
    }

    async fn credentials(&self) -> credentials::Result {
        tracing::debug!(command = %self.credential_process, "loading credentials from external process");

        let mut command = parse_command_str(&self.credential_process)?;

        tracing::debug!(command = ?command, "parsed command");

        let output = command.output().map_err(|e| {
            CredentialsError::provider_error(format!(
                "Error retrieving credentials from external process: {}",
                e
            ))
        })?;

        tracing::debug!(command = ?command, status = ?output.status, "executed command");

        if !output.status.success() {
            return Err(CredentialsError::provider_error(format!(
                "Error retrieving credentials from external process: exited with code: {}",
                output.status
            )));
        }

        let output = std::str::from_utf8(&output.stdout).map_err(|e| {
            CredentialsError::provider_error(format!(
                "Error retrieving credentials from external process: could not decode output as UTF-8: {}",
                e
            ))
        })?;

        match parse_json_credentials(&output) {
            Ok(JsonCredentials::RefreshableCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                expiration,
                ..
            }) => Ok(Credentials::new(
                access_key_id,
                secret_access_key,
                Some(session_token.to_string()),
                expiration.into(),
                "IMDSv2",
            )),
            Ok(JsonCredentials::Error { code, message }) => {
                Err(CredentialsError::provider_error(format!(
                    "Error retrieving credentials from external process: {} {}",
                    code, message
                )))
            }
            Err(invalid) => Err(CredentialsError::provider_error(format!(
                "Error retrieving credentials from external process, could not parse response: {}",
                invalid
            ))),
        }
    }
}

fn parse_command_str(s: &str) -> Result<Command, CredentialsError> {
    let args = shlex::split(s).ok_or_else(|| {
        CredentialsError::provider_error(
            "Error retrieving credentials from external process: could not parse provided value as command",
        )
    })?;
    let mut iter = args.iter();
    let mut command = Command::new(iter.next().ok_or_else(|| {
        CredentialsError::provider_error(
            "Error retrieving credentials from external process: provided command empty",
        )
    })?);
    command.args(iter);
    Ok(command)
}

#[cfg(test)]
mod test {
    // TODO
}
