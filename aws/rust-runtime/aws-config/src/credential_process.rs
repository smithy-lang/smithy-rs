/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Credentials Provider for external process

use crate::json_credentials::{json_parse_loop, InvalidJsonCredentials, RefreshableCredentials};
use aws_smithy_json::deserialize::Token;
use aws_smithy_types::date_time::Format;
use aws_smithy_types::DateTime;
use aws_types::credentials::{future, CredentialsError, ProvideCredentials};
use aws_types::{credentials, Credentials};
use std::process::Command;
use std::time::SystemTime;

/// Credentials Provider
#[derive(Debug)]
pub struct CredentialProcessProvider {
    command: String,
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
    pub fn new(command: String) -> Self {
        Self { command }
    }

    async fn credentials(&self) -> credentials::Result {
        tracing::debug!(command = %self.command, "loading credentials from external process");

        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd.exe");
            command.args(&["/C", &self.command]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(&["-c", &self.command]);
            command
        };

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

        match parse_credential_process_json_credentials(&output) {
            Ok(RefreshableCredentials {
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
                "CredentialProcess",
            )),
            Err(invalid) => Err(CredentialsError::provider_error(format!(
                "Error retrieving credentials from external process, could not parse response: {}",
                invalid
            ))),
        }
    }
}

/// Deserialize a credential_process response from a string
///
/// Returns an error if the response cannot be successfully parsed or is missing keys.
///
/// Keys are case insensitive.
pub(crate) fn parse_credential_process_json_credentials(
    credentials_response: &str,
) -> Result<RefreshableCredentials, InvalidJsonCredentials> {
    let mut version = None;
    let mut access_key_id = None;
    let mut secret_access_key = None;
    let mut session_token = None;
    let mut expiration = None;
    json_parse_loop(credentials_response.as_bytes(), |key, value| {
        match (key, value) {
            /*
             "Version": 1,
             "AccessKeyId": "ASIARTESTID",
             "SecretAccessKey": "TESTSECRETKEY",
             "SessionToken": "TESTSESSIONTOKEN",
             "Expiration": "2022-05-02T18:36:00+00:00"
            */
            (key, Token::ValueNumber { value, .. }) if key.eq_ignore_ascii_case("Version") => {
                version = Some(value.to_u8())
            }
            (key, Token::ValueString { value, .. }) if key.eq_ignore_ascii_case("AccessKeyId") => {
                access_key_id = Some(value.to_unescaped()?)
            }
            (key, Token::ValueString { value, .. })
                if key.eq_ignore_ascii_case("SecretAccessKey") =>
            {
                secret_access_key = Some(value.to_unescaped()?)
            }
            (key, Token::ValueString { value, .. }) if key.eq_ignore_ascii_case("SessionToken") => {
                session_token = Some(value.to_unescaped()?)
            }
            (key, Token::ValueString { value, .. }) if key.eq_ignore_ascii_case("Expiration") => {
                expiration = Some(value.to_unescaped()?)
            }

            _ => {}
        };
        Ok(())
    })?;

    match version {
        Some(1) => {
            let access_key_id =
                access_key_id.ok_or(InvalidJsonCredentials::MissingField("AccessKeyId"))?;
            let secret_access_key =
                secret_access_key.ok_or(InvalidJsonCredentials::MissingField("SecretAccessKey"))?;
            let session_token =
                session_token.ok_or(InvalidJsonCredentials::MissingField("Token"))?;
            let expiration =
                expiration.ok_or(InvalidJsonCredentials::MissingField("Expiration"))?;
            let expiration = SystemTime::try_from(
                DateTime::from_str(expiration.as_ref(), Format::DateTime).map_err(|err| {
                    InvalidJsonCredentials::InvalidField {
                        field: "Expiration",
                        err: err.into(),
                    }
                })?,
            )
            .map_err(|_| {
                InvalidJsonCredentials::Other(
                    "credential expiration time cannot be represented by a SystemTime".into(),
                )
            })?;
            Ok(RefreshableCredentials {
                access_key_id,
                secret_access_key,
                session_token,
                expiration,
            })
        }
        None => Err(InvalidJsonCredentials::MissingField("Version")),
        Some(version) => Err(InvalidJsonCredentials::InvalidField {
            field: "version",
            err: format!("unknown version number: {}", version).into(),
        }),
    }
}

#[cfg(test)]
mod test {
    use crate::credential_process::CredentialProcessProvider;
    use aws_smithy_types::date_time::Format;
    use aws_smithy_types::DateTime;
    use aws_types::credentials::ProvideCredentials;
    use std::time::SystemTime;

    #[tokio::test]
    async fn test_credential_process() {
        let provider = CredentialProcessProvider::new(String::from(
            r#"echo '{ "Version": 1, "AccessKeyId": "ASIARTESTID", "SecretAccessKey": "TESTSECRETKEY", "SessionToken": "TESTSESSIONTOKEN", "Expiration": "2022-05-02T18:36:00+00:00" }'"#,
        ));
        let creds = provider.provide_credentials().await.expect("valid creds");
        assert_eq!(creds.access_key_id(), "ASIARTESTID");
        assert_eq!(creds.secret_access_key(), "TESTSECRETKEY");
        assert_eq!(creds.session_token(), Some("TESTSESSIONTOKEN"));
        assert_eq!(
            creds.expiry(),
            Some(
                SystemTime::try_from(
                    DateTime::from_str("2022-05-02T18:36:00+00:00", Format::DateTime)
                        .expect("static datetime")
                )
                .expect("static datetime")
            )
        );
    }
}
