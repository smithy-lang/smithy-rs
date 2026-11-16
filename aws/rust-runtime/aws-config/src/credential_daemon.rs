/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Credentials provider for a local credential daemon
//!
//! A credential daemon is a local process that holds credentials and hands them out over a
//! Unix domain socket. The SDK sends one JSON-RPC 2.0 `credential/get` request per fetch, on a
//! connection of its own, and reads one newline-terminated reply.
//!
//! This provider is part of the profile credentials provider. A profile in the shared config
//! file selects it with the `daemon_credential` key:
//!
//! ```ini
//! [profile dev]
//! daemon_credential = dev
//! ```
//!
//! The socket is looked up on every fetch, and only the first of these locations that applies
//! is used:
//! 1. `AWS_CREDENTIAL_DAEMON_SOCKET`, if it is an absolute path. An empty or relative value
//!    counts as unset.
//! 2. `$HOME/.aws/credential-daemon.sock`, if anything is there: a socket, a symbolic link, or
//!    a link whose target is missing. `HOME` is read from the environment, and this location
//!    is skipped when `HOME` is unset, empty or relative.
//!
//! When no location applies, the provider fails with NOT_RUNNING without dialing. When location
//! 2 is a link whose target is missing, the NOT_RUNNING error names the link and its target.
//! Otherwise the provider dials only the location it found, and a NOT_RUNNING error names it
//! (for location 2, the link's path, not its target).
//!
//! The provider needs the `credential-daemon` cargo feature and a Unix platform. Without them,
//! a profile that sets `daemon_credential` fails with
//! [`CredentialDaemonErrorCode::UnsupportedPlatform`].

use aws_credential_types::provider::{self, error::CredentialsError, future, ProvideCredentials};
use std::error::Error;
use std::fmt;

#[cfg(all(feature = "credential-daemon", unix))]
pub use provider_impl::{CredentialDaemonBuilder, CredentialDaemonProvider};

/// Longest daemon message that is kept in an error, in characters.
const MAX_DAEMON_MESSAGE_CHARS: usize = 1024;
/// The most characters of a material type from the credential daemon kept in an error message.
#[allow(dead_code)] // unused without the `credential-daemon` feature
const MAX_MATERIAL_TYPE_CHARS: usize = 64;

/// The kind of a [`CredentialDaemonError`].
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialDaemonErrorCode {
    /// The credential daemon could not be reached, or it did not reply in time.
    NotRunning,
    /// The credential daemon rejected the request.
    RequestRejected,
    /// The credential daemon needs the user to sign in again.
    ReauthRequired,
    /// The credential daemon does not know the requested credential.
    CredentialNotFound,
    /// The credential daemon returned a kind of credential material this SDK does not support.
    UnsupportedMaterial,
    /// The reply of the credential daemon could not be understood.
    ProtocolError,
    /// The profile that selects the credential daemon is not valid.
    InvalidConfig,
    /// This build of the SDK, or this platform, can not talk to a credential daemon.
    UnsupportedPlatform,
}

impl CredentialDaemonErrorCode {
    /// The code as a string, for example `NOT_RUNNING`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotRunning => "NOT_RUNNING",
            Self::RequestRejected => "REQUEST_REJECTED",
            Self::ReauthRequired => "REAUTH_REQUIRED",
            Self::CredentialNotFound => "CREDENTIAL_NOT_FOUND",
            Self::UnsupportedMaterial => "UNSUPPORTED_MATERIAL",
            Self::ProtocolError => "PROTOCOL_ERROR",
            Self::InvalidConfig => "INVALID_CONFIG",
            Self::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
        }
    }

    /// True if retrying a fetch that failed with this code may succeed.
    #[allow(dead_code)] // unused without the `credential-daemon` feature
    pub(crate) fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::NotRunning | Self::RequestRejected | Self::ProtocolError
        )
    }
}

impl fmt::Display for CredentialDaemonErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error from getting credentials from a credential daemon.
///
/// See the [module documentation](crate::credential_daemon) for the profile key, the socket
/// lookup and what each [`CredentialDaemonErrorCode`] means.
#[derive(Clone, Debug)]
pub struct CredentialDaemonError {
    code: CredentialDaemonErrorCode,
    message: String,
}

impl CredentialDaemonError {
    pub(crate) fn new(code: CredentialDaemonErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn invalid_config(message: impl Into<String>) -> Self {
        Self::new(CredentialDaemonErrorCode::InvalidConfig, message)
    }

    #[allow(dead_code)] // unused with the `credential-daemon` feature on Unix
    pub(crate) fn unsupported_platform(profile: &str) -> Self {
        Self::new(
            CredentialDaemonErrorCode::UnsupportedPlatform,
            format!(
                "profile `{profile}` sets `daemon_credential`, but this build of the SDK can not \
                 talk to a credential daemon. It needs the `credential-daemon` feature of \
                 `aws-config` and a Unix platform."
            ),
        )
    }

    /// The kind of error.
    pub fn code(&self) -> CredentialDaemonErrorCode {
        self.code
    }
}

impl fmt::Display for CredentialDaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} See the `aws_config::credential_daemon` module docs.",
            self.code, self.message
        )
    }
}

impl Error for CredentialDaemonError {}

/// True if `name` is a valid credential daemon credential name.
///
/// A name is 1 to 64 characters from `A-Z`, `a-z`, `0-9`, `.`, `_` and `-`, and does not start
/// with `.`.
pub(crate) fn validate_credential_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Drops control characters from a message of the credential daemon, and shortens it.
#[allow(dead_code)] // unused without the `credential-daemon` feature
fn sanitize(message: &str) -> String {
    sanitize_to(message, MAX_DAEMON_MESSAGE_CHARS)
}

/// Drops control characters from text of the credential daemon, and keeps at most `max` characters.
#[allow(dead_code)] // unused without the `credential-daemon` feature
fn sanitize_to(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// A credentials provider that always fails with the same credential daemon error.
///
/// It stands in for the credential daemon provider when a daemon profile is not valid, or when
/// this build can not talk to a credential daemon, so that every call reports the error.
#[derive(Debug)]
pub(crate) struct FailClosedProvider(pub(crate) CredentialDaemonError);

impl ProvideCredentials for FailClosedProvider {
    fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        let result: provider::Result = Err(CredentialsError::provider_error(self.0.clone()));
        future::ProvideCredentials::ready(result)
    }
}

#[cfg(all(feature = "credential-daemon", unix))]
mod provider_impl {
    use super::{
        sanitize, sanitize_to, validate_credential_name, CredentialDaemonError,
        CredentialDaemonErrorCode as Code, MAX_MATERIAL_TYPE_CHARS,
    };
    use crate::provider_config::ProviderConfig;
    use aws_credential_types::attributes::AccountId;
    use aws_credential_types::credential_feature::AwsCredentialFeature;
    use aws_credential_types::provider::{
        self, error::CredentialsError, future, ProvideCredentials,
    };
    use aws_credential_types::Credentials;
    use aws_smithy_async::future::timeout::Timeout;
    use aws_smithy_async::rt::sleep::{AsyncSleep, TokioSleep};
    use aws_smithy_json::deserialize::token::{skip_to_end, skip_value};
    use aws_smithy_json::deserialize::{json_token_iter, Token};
    use aws_smithy_json::serialize::JsonObjectWriter;
    use aws_smithy_types::date_time::Format;
    use aws_smithy_types::{DateTime, Number};
    use aws_types::os_shim_internal::Env;
    use std::future::Future;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tokio::net::UnixStream;

    pub(super) const SOCKET_ENV_VAR: &str = "AWS_CREDENTIAL_DAEMON_SOCKET";
    /// Location 2, relative to `HOME`.
    const HOME_SOCKET: &str = ".aws/credential-daemon.sock";

    /// Longest reply line, in bytes, not counting the newline.
    pub(super) const MAX_REPLY_BYTES: usize = 64_000;
    const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
    const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(5);
    const PROVIDER_NAME: &str = "CredentialDaemon";
    const SESSION_CREDENTIALS: &str = "session_credentials";
    /// The most characters of a daemon `data.code` kept in an error message.
    const MAX_DAEMON_CODE_CHARS: usize = 64;

    /// Credentials provider that gets credentials from a local credential daemon
    ///
    /// See the [module documentation](crate::credential_daemon) for the socket lookup.
    #[derive(Debug)]
    pub struct CredentialDaemonProvider {
        credential: String,
        sso_role: Option<(String, String)>,
        env: Env,
        connect_timeout: Duration,
        call_timeout: Duration,
    }

    impl CredentialDaemonProvider {
        /// Builder for this credentials provider.
        pub fn builder() -> CredentialDaemonBuilder {
            CredentialDaemonBuilder::default()
        }

        async fn credentials(&self) -> provider::Result {
            self.fetch().await.map_err(CredentialsError::provider_error)
        }

        async fn fetch(&self) -> Result<Credentials, CredentialDaemonError> {
            if !validate_credential_name(&self.credential) {
                return Err(CredentialDaemonError::invalid_config(
                    "the credential name is not valid. A credential daemon credential name is \
                     1 to 64 characters from `A-Z`, `a-z`, `0-9`, `.`, `_` and `-`, and does \
                     not start with `.`.",
                ));
            }
            let request = request_line(
                &self.credential,
                self.sso_role
                    .as_ref()
                    .map(|(account, role)| (account.as_str(), role.as_str())),
            );
            let path = resolve_socket(&self.env)?;
            tracing::debug!(socket = %path.display(), credential = %self.credential, "loading credentials from the credential daemon");
            let reply = call(
                &path,
                request.as_bytes(),
                self.connect_timeout,
                self.call_timeout,
            )
            .await?;
            parse_reply(&reply, &self.credential)
        }
    }

    impl ProvideCredentials for CredentialDaemonProvider {
        fn provide_credentials<'a>(&'a self) -> future::ProvideCredentials<'a>
        where
            Self: 'a,
        {
            future::ProvideCredentials::new(self.credentials())
        }
    }

    /// Builder for [`CredentialDaemonProvider`]
    #[derive(Debug, Default)]
    pub struct CredentialDaemonBuilder {
        provider_config: Option<ProviderConfig>,
        credential: Option<String>,
        sso_role: Option<(String, String)>,
        connect_timeout: Option<Duration>,
        call_timeout: Option<Duration>,
    }

    impl CredentialDaemonBuilder {
        /// Override the configuration used by this provider, such as its environment
        /// variables.
        pub fn configure(mut self, provider_config: &ProviderConfig) -> Self {
            self.provider_config = Some(provider_config.clone());
            self
        }

        /// The name of the credential to ask the credential daemon for.
        pub fn credential(mut self, name: impl Into<String>) -> Self {
            self.credential = Some(name.into());
            self
        }

        /// The SSO account ID and role name to send with the request.
        pub fn sso_role(
            mut self,
            account_id: impl Into<String>,
            role_name: impl Into<String>,
        ) -> Self {
            self.sso_role = Some((account_id.into(), role_name.into()));
            self
        }

        /// How long to wait for the connection to the credential daemon. Defaults to 1 second.
        pub fn connect_timeout(mut self, timeout: Duration) -> Self {
            self.connect_timeout = Some(timeout);
            self
        }

        /// How long to wait for the whole call, from connecting to reading the reply.
        /// Defaults to 5 seconds.
        pub fn call_timeout(mut self, timeout: Duration) -> Self {
            self.call_timeout = Some(timeout);
            self
        }

        /// Builds a [`CredentialDaemonProvider`].
        ///
        /// The credential name is checked on every fetch, which fails with
        /// [`INVALID_CONFIG`](super::CredentialDaemonErrorCode::InvalidConfig) when it is not
        /// valid.
        pub fn build(self) -> CredentialDaemonProvider {
            let conf = self.provider_config.unwrap_or_default();
            CredentialDaemonProvider {
                credential: self.credential.unwrap_or_default(),
                sso_role: self.sso_role,
                env: conf.env(),
                connect_timeout: self.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT),
                call_timeout: self.call_timeout.unwrap_or(DEFAULT_CALL_TIMEOUT),
            }
        }
    }

    /// An environment variable, when it is set, Unicode and not empty.
    fn env_value(env: &Env, key: &str) -> Option<String> {
        env.get(key).ok().filter(|value| !value.is_empty())
    }

    /// Finds the socket of the credential daemon. Only the first location that applies is
    /// used, and nothing is tried after a failed dial.
    pub(super) fn resolve_socket(env: &Env) -> Result<PathBuf, CredentialDaemonError> {
        if let Some(path) = env_value(env, SOCKET_ENV_VAR) {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                return Ok(path);
            }
            tracing::debug!(
                "`{SOCKET_ENV_VAR}` ignored because it is not an absolute path: {}",
                path.display()
            );
        }
        let Some(link) = env_value(env, "HOME")
            .map(|home| PathBuf::from(home).join(HOME_SOCKET))
            .filter(|path| path.is_absolute())
        else {
            return Err(no_socket());
        };
        // `symlink_metadata` doesn't follow a link, so a dangling link still applies.
        match std::fs::symlink_metadata(&link) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if let Err(err) = std::fs::metadata(&link) {
                    if err.kind() == io::ErrorKind::NotFound {
                        let target = std::fs::read_link(&link)
                            .map(|target| target.display().to_string())
                            .unwrap_or_else(|_| "an unknown path".to_string());
                        return Err(not_running(format!(
                            "`{}` is a link to `{target}`, which does not exist. Start the \
                             credential daemon, or link its socket to `$HOME/{HOME_SOCKET}`.",
                            link.display()
                        )));
                    }
                }
                Ok(link)
            }
            Ok(_) => Ok(link),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                Err(no_socket())
            }
            // Something may be there. Dial it, and let the dial report the error.
            Err(_) => Ok(link),
        }
    }

    /// The NOT_RUNNING error for no location that applies.
    fn no_socket() -> CredentialDaemonError {
        not_running(format!(
            "no credential daemon socket was found. Install the credential daemon or link its \
             socket to `$HOME/{HOME_SOCKET}`, or set `{SOCKET_ENV_VAR}` to the absolute path of \
             its socket."
        ))
    }

    fn not_running(message: String) -> CredentialDaemonError {
        CredentialDaemonError::new(Code::NotRunning, message)
    }

    fn protocol_error(reason: &str) -> CredentialDaemonError {
        CredentialDaemonError::new(
            Code::ProtocolError,
            format!("the reply of the credential daemon could not be used: {reason}."),
        )
    }

    /// Sends one request line to the credential daemon at `path`, and returns its reply line
    /// without the newline.
    pub(super) async fn call(
        path: &Path,
        request: &[u8],
        connect_timeout: Duration,
        call_timeout: Duration,
    ) -> Result<Vec<u8>, CredentialDaemonError> {
        let inner = async {
            let stream =
                connect_with_timeout(path, UnixStream::connect(path), connect_timeout).await?;
            exchange(&stream, path, request).await
        };
        match Timeout::new(inner, TokioSleep::new().sleep(call_timeout)).await {
            Ok(result) => result,
            Err(_timed_out) => Err(not_running(format!(
                "the credential daemon at `{}` did not reply within {call_timeout:?} (call timeout).",
                path.display()
            ))),
        }
    }

    pub(super) async fn connect_with_timeout<T>(
        path: &Path,
        connect: impl Future<Output = io::Result<T>>,
        connect_timeout: Duration,
    ) -> Result<T, CredentialDaemonError> {
        match Timeout::new(connect, TokioSleep::new().sleep(connect_timeout)).await {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(err)) => Err(not_running(format!(
                "could not connect to the credential daemon at `{}` ({}).",
                path.display(),
                err.kind()
            ))),
            Err(_timed_out) => Err(not_running(format!(
                "could not connect to the credential daemon at `{}` within {connect_timeout:?} (connect timeout).",
                path.display()
            ))),
        }
    }

    async fn exchange(
        stream: &UnixStream,
        path: &Path,
        request: &[u8],
    ) -> Result<Vec<u8>, CredentialDaemonError> {
        let io_error = |err: io::Error| {
            not_running(format!(
                "the connection to the credential daemon at `{}` failed ({}).",
                path.display(),
                err.kind()
            ))
        };

        let mut written = 0;
        while written < request.len() {
            stream.writable().await.map_err(io_error)?;
            match stream.try_write(&request[written..]) {
                Ok(0) => return Err(io_error(io::ErrorKind::WriteZero.into())),
                Ok(n) => written += n,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) => return Err(io_error(err)),
            }
        }

        // The reply never holds more than `MAX_REPLY_BYTES + 1` bytes.
        let mut reply = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            stream.readable().await.map_err(io_error)?;
            let room = (MAX_REPLY_BYTES + 1 - reply.len()).min(buf.len());
            match stream.try_read(&mut buf[..room]) {
                Ok(0) if reply.is_empty() => {
                    return Err(protocol_error("closed the connection without replying"))
                }
                Ok(0) => return Err(protocol_error("reply was not newline-terminated")),
                Ok(n) => {
                    let chunk = &buf[..n];
                    let newline = chunk.iter().position(|b| *b == b'\n');
                    reply.extend_from_slice(&chunk[..newline.unwrap_or(n)]);
                    if reply.len() > MAX_REPLY_BYTES {
                        return Err(protocol_error(&format!(
                            "reply exceeded {MAX_REPLY_BYTES} bytes"
                        )));
                    }
                    if newline.is_some() {
                        return Ok(reply);
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) => return Err(io_error(err)),
            }
        }
    }

    /// The `credential/get` request line, newline included.
    pub(super) fn request_line(credential: &str, sso_role: Option<(&str, &str)>) -> String {
        let mut out = String::new();
        let mut request = JsonObjectWriter::new(&mut out);
        request.key("jsonrpc").string("2.0");
        request.key("id").number(Number::PosInt(1));
        request.key("method").string("credential/get");
        let mut params = request.key("params").start_object();
        params.key("credential").string(credential);
        if let Some((account_id, role_name)) = sso_role {
            params.key("account_id").string(account_id);
            params.key("role_name").string(role_name);
        }
        params.finish();
        request.finish();
        out.push('\n');
        out
    }

    type Tokens<'a> = std::iter::Peekable<aws_smithy_json::deserialize::JsonTokenIterator<'a>>;

    const NOT_JSON: &str = "reply is not valid JSON";

    fn next_token<'a>(tokens: &mut Tokens<'a>) -> Result<Option<Token<'a>>, CredentialDaemonError> {
        tokens
            .next()
            .transpose()
            .map_err(|_| protocol_error(NOT_JSON))
    }

    fn skip(tokens: &mut Tokens<'_>) -> Result<(), CredentialDaemonError> {
        skip_value(tokens).map_err(|_| protocol_error(NOT_JSON))
    }

    /// Reads the next object key, or `None` at the end of the object.
    fn next_key(tokens: &mut Tokens<'_>) -> Result<Option<String>, CredentialDaemonError> {
        match next_token(tokens)? {
            Some(Token::EndObject { .. }) => Ok(None),
            Some(Token::ObjectKey { key, .. }) => key
                .to_unescaped()
                .map(|key| Some(key.into_owned()))
                .map_err(|_| protocol_error(NOT_JSON)),
            _ => Err(protocol_error(NOT_JSON)),
        }
    }

    /// Reads a string value. `null` reads as `None`, and any other value is an error.
    fn optional_string(
        tokens: &mut Tokens<'_>,
        wrong_type: &'static str,
    ) -> Result<Option<String>, CredentialDaemonError> {
        match next_token(tokens)? {
            Some(Token::ValueString { value, .. }) => value
                .to_unescaped()
                .map(|value| Some(value.into_owned()))
                .map_err(|_| protocol_error(NOT_JSON)),
            Some(Token::ValueNull { .. }) => Ok(None),
            Some(Token::StartObject { .. }) | Some(Token::StartArray { .. }) => {
                skip_to_end(tokens).map_err(|_| protocol_error(NOT_JSON))?;
                Err(protocol_error(wrong_type))
            }
            Some(_) => Err(protocol_error(wrong_type)),
            None => Err(protocol_error(NOT_JSON)),
        }
    }

    /// Consumes the start of an object, or returns false (and skips the value) for `null`.
    fn start_object(
        tokens: &mut Tokens<'_>,
        wrong_type: &'static str,
    ) -> Result<bool, CredentialDaemonError> {
        match next_token(tokens)? {
            Some(Token::StartObject { .. }) => Ok(true),
            Some(Token::ValueNull { .. }) => Ok(false),
            Some(Token::StartArray { .. }) => {
                skip_to_end(tokens).map_err(|_| protocol_error(NOT_JSON))?;
                Err(protocol_error(wrong_type))
            }
            Some(_) => Err(protocol_error(wrong_type)),
            None => Err(protocol_error(NOT_JSON)),
        }
    }

    #[derive(Default)]
    struct Material {
        kind: Option<String>,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        session_token: Option<String>,
        account_id: Option<String>,
    }

    #[derive(Default)]
    struct ReplyResult {
        expires_at: Option<String>,
        material: Option<Material>,
    }

    #[derive(Default)]
    struct ReplyError {
        message: Option<String>,
        code: Option<String>,
    }

    fn parse_material(tokens: &mut Tokens<'_>) -> Result<Material, CredentialDaemonError> {
        let mut material = Material::default();
        const WRONG: &str = "a `result.material` field is not a string";
        while let Some(key) = next_key(tokens)? {
            match key.as_str() {
                "type" => material.kind = optional_string(tokens, WRONG)?,
                "access_key_id" => material.access_key_id = optional_string(tokens, WRONG)?,
                "secret_access_key" => material.secret_access_key = optional_string(tokens, WRONG)?,
                "session_token" => material.session_token = optional_string(tokens, WRONG)?,
                "account_id" => material.account_id = optional_string(tokens, WRONG)?,
                _ => skip(tokens)?,
            }
        }
        Ok(material)
    }

    fn parse_result(tokens: &mut Tokens<'_>) -> Result<ReplyResult, CredentialDaemonError> {
        let mut result = ReplyResult::default();
        while let Some(key) = next_key(tokens)? {
            match key.as_str() {
                "expires_at" => {
                    result.expires_at =
                        optional_string(tokens, "`result.expires_at` is not a string")?
                }
                "material" => {
                    result.material = if start_object(tokens, "`result.material` is not an object")?
                    {
                        Some(parse_material(tokens)?)
                    } else {
                        None
                    }
                }
                _ => skip(tokens)?,
            }
        }
        Ok(result)
    }

    fn parse_error(tokens: &mut Tokens<'_>) -> Result<ReplyError, CredentialDaemonError> {
        let mut error = ReplyError::default();
        while let Some(key) = next_key(tokens)? {
            match key.as_str() {
                "message" => {
                    error.message = optional_string(tokens, "`error.message` is not a string")?
                }
                "data" => {
                    if matches!(tokens.peek(), Some(Ok(Token::StartObject { .. }))) {
                        next_token(tokens)?;
                        while let Some(key) = next_key(tokens)? {
                            match key.as_str() {
                                "code" => {
                                    error.code = optional_string(
                                        tokens,
                                        "`error.data.code` is not a string",
                                    )?
                                }
                                _ => skip(tokens)?,
                            }
                        }
                    } else {
                        skip(tokens)?
                    }
                }
                _ => skip(tokens)?,
            }
        }
        Ok(error)
    }

    /// Parses `expires_at`: an RFC 3339 timestamp in UTC (`Z`), with 0 to 9 fractional digits.
    fn parse_expires_at(expires_at: &str) -> Option<SystemTime> {
        let without_zone = expires_at.strip_suffix('Z')?;
        if let Some((_, fraction)) = without_zone.split_once('.') {
            if !(1..=9).contains(&fraction.len()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
        }
        let date_time = DateTime::from_str(expires_at, Format::DateTime).ok()?;
        SystemTime::try_from(date_time).ok()
    }

    /// Parses the reply line of the credential daemon.
    ///
    /// No error from here holds content of the reply, except the sanitized error message of the
    /// daemon.
    pub(super) fn parse_reply(
        line: &[u8],
        credential: &str,
    ) -> Result<Credentials, CredentialDaemonError> {
        let line =
            std::str::from_utf8(line).map_err(|_| protocol_error("reply is not valid UTF-8"))?;
        let mut tokens = json_token_iter(line.as_bytes()).peekable();
        if !matches!(next_token(&mut tokens)?, Some(Token::StartObject { .. })) {
            return Err(protocol_error("reply is not a JSON object"));
        }
        let mut jsonrpc_ok = false;
        let mut result = None;
        let mut error = None;
        while let Some(key) = next_key(&mut tokens)? {
            match key.as_str() {
                "jsonrpc" => {
                    jsonrpc_ok = matches!(
                        tokens.peek(),
                        Some(Ok(Token::ValueString { value, .. })) if value.as_escaped_str() == "2.0"
                    );
                    skip(&mut tokens)?;
                }
                // A `null` `result` or `error` counts as absent.
                "result" => {
                    if start_object(&mut tokens, "`result` is not an object")? {
                        result = Some(parse_result(&mut tokens)?);
                    }
                }
                "error" => {
                    if start_object(&mut tokens, "`error` is not an object")? {
                        error = Some(parse_error(&mut tokens)?);
                    }
                }
                _ => skip(&mut tokens)?,
            }
        }
        if next_token(&mut tokens)?.is_some() {
            return Err(protocol_error("reply holds more than one JSON value"));
        }
        if !jsonrpc_ok {
            return Err(protocol_error("`jsonrpc` is not \"2.0\""));
        }
        match (result, error) {
            (Some(_), Some(_)) => Err(protocol_error("reply holds both `result` and `error`")),
            (None, None) => Err(protocol_error("reply holds neither `result` nor `error`")),
            (None, Some(error)) => Err(error_reply(error, credential)),
            (Some(result), None) => credentials_from_result(result),
        }
    }

    fn error_reply(error: ReplyError, credential: &str) -> CredentialDaemonError {
        let code = match error.code.as_deref() {
            Some("REAUTH_REQUIRED") => Code::ReauthRequired,
            Some("CREDENTIAL_NOT_FOUND") => Code::CredentialNotFound,
            _ => Code::RequestRejected,
        };
        let mut message =
            format!("the credential daemon could not provide credential `{credential}`");
        if code == Code::RequestRejected {
            if let Some(daemon_code) = error
                .code
                .as_deref()
                .filter(|daemon_code| *daemon_code != "REQUEST_REJECTED")
            {
                message.push_str(&format!(
                    " (daemon code `{}`)",
                    sanitize_to(daemon_code, MAX_DAEMON_CODE_CHARS)
                ));
            }
        }
        if let Some(daemon_message) = error.message {
            message.push_str(": ");
            message.push_str(&sanitize(&daemon_message));
        }
        message.push('.');
        CredentialDaemonError::new(code, message)
    }

    fn credentials_from_result(result: ReplyResult) -> Result<Credentials, CredentialDaemonError> {
        let material = result
            .material
            .ok_or_else(|| protocol_error("missing `result.material`"))?;
        let kind = material
            .kind
            .ok_or_else(|| protocol_error("missing `result.material.type`"))?;
        if kind != SESSION_CREDENTIALS {
            return Err(CredentialDaemonError::new(
                Code::UnsupportedMaterial,
                format!(
                    "this SDK version doesn't support material type `{}` from the credential \
                     daemon. Update the SDK.",
                    sanitize_to(&kind, MAX_MATERIAL_TYPE_CHARS)
                ),
            ));
        }
        let access_key_id = material
            .access_key_id
            .filter(|value| !value.is_empty())
            .ok_or_else(|| protocol_error("missing `result.material.access_key_id`"))?;
        let secret_access_key = material
            .secret_access_key
            .filter(|value| !value.is_empty())
            .ok_or_else(|| protocol_error("missing `result.material.secret_access_key`"))?;
        let expires_at = result
            .expires_at
            .ok_or_else(|| protocol_error("missing `result.expires_at`"))?;
        let expiry = parse_expires_at(&expires_at).ok_or_else(|| {
            protocol_error("`result.expires_at` is not an RFC 3339 timestamp in UTC")
        })?;

        let mut builder = Credentials::builder()
            .access_key_id(access_key_id)
            .secret_access_key(secret_access_key)
            .expiry(expiry)
            .provider_name(PROVIDER_NAME);
        builder.set_session_token(material.session_token.filter(|value| !value.is_empty()));
        builder.set_account_id(
            material
                .account_id
                .filter(|value| !value.is_empty())
                .map(AccountId::from),
        );
        let mut credentials = builder.build();
        credentials
            .get_property_mut_or_default::<Vec<AwsCredentialFeature>>()
            .push(AwsCredentialFeature::CredentialDaemon);
        Ok(credentials)
    }
}

#[cfg(test)]
mod test {
    use super::{sanitize, validate_credential_name, CredentialDaemonErrorCode};

    #[test]
    fn credential_names() {
        for valid in ["a", "dev", "A.b_c-1", "dev.", "0", &"a".repeat(64)] {
            assert!(validate_credential_name(valid), "{valid}");
        }
        for invalid in ["", ".dev", "a/b", "a b", "dév", "a\n", &"a".repeat(65)] {
            assert!(!validate_credential_name(invalid), "{invalid}");
        }
    }

    #[test]
    fn sanitize_drops_control_characters_and_shortens() {
        assert_eq!("bad thing", sanitize("\u{7}bad\n \u{1b}thing\u{9b}\r"));
        assert_eq!(1024, sanitize(&"é".repeat(2000)).chars().count());
    }

    #[test]
    fn recoverable_codes() {
        use CredentialDaemonErrorCode::*;
        for code in [NotRunning, RequestRejected, ProtocolError] {
            assert!(code.is_recoverable(), "{code}");
        }
        for code in [
            ReauthRequired,
            CredentialNotFound,
            UnsupportedMaterial,
            InvalidConfig,
            UnsupportedPlatform,
        ] {
            assert!(!code.is_recoverable(), "{code}");
        }
    }

    #[test]
    fn display_has_code_and_doc_link() {
        let err = super::CredentialDaemonError::invalid_config("bad profile.");
        assert_eq!(
            "INVALID_CONFIG: bad profile. See the `aws_config::credential_daemon` module docs.",
            err.to_string()
        );
        assert_eq!(CredentialDaemonErrorCode::InvalidConfig, err.code());
    }
}

#[cfg(all(test, feature = "credential-daemon", unix))]
mod provider_test {
    use super::provider_impl::{
        call, connect_with_timeout, parse_reply, request_line, resolve_socket, MAX_REPLY_BYTES,
        SOCKET_ENV_VAR,
    };
    use super::{
        CredentialDaemonError, CredentialDaemonErrorCode as Code, CredentialDaemonProvider,
    };
    use crate::provider_config::ProviderConfig;
    use aws_credential_types::credential_feature::AwsCredentialFeature;
    use aws_credential_types::provider::error::CredentialsError;
    use aws_credential_types::provider::ProvideCredentials;
    use aws_credential_types::provider::SharedCredentialsProvider;
    use aws_credential_types::Credentials;
    use aws_smithy_async::rt::sleep::TokioSleep;
    use aws_smithy_async::test_util::ManualTimeSource;
    use aws_smithy_runtime::client::identity::IdentityCache;
    use aws_smithy_runtime_api::box_error::BoxError;
    use aws_smithy_runtime_api::client::identity::{
        ResolveCachedIdentity, SharedIdentityCache, SharedIdentityResolver,
    };
    use aws_smithy_runtime_api::client::runtime_components::{
        RuntimeComponents, RuntimeComponentsBuilder,
    };
    use aws_smithy_types::config_bag::ConfigBag;
    use aws_smithy_types::date_time::Format;
    use aws_smithy_types::DateTime;
    use aws_types::os_shim_internal::Env;
    use std::error::Error;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    /// A temporary directory, removed on drop
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "cd-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What the fake credential daemon does on a connection, after reading the request line
    #[derive(Clone)]
    enum Behavior {
        /// Write these bytes, then close
        Reply(Vec<u8>),
        /// Never reply
        Silent,
    }

    fn reply_line(line: impl Into<String>) -> Behavior {
        let mut line = line.into();
        line.push('\n');
        Behavior::Reply(line.into_bytes())
    }

    struct FakeDaemon {
        socket: PathBuf,
        requests: Arc<Mutex<Vec<String>>>,
        _dir: TempDir,
    }

    impl FakeDaemon {
        /// Serves `behaviors` in order, one per connection. The last one repeats.
        fn start(behaviors: Vec<Behavior>) -> Self {
            let dir = TempDir::new();
            let socket = dir.path().join("daemon.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let recorded = requests.clone();
            tokio::spawn(async move {
                let mut index = 0;
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let behavior = behaviors[index.min(behaviors.len() - 1)].clone();
                    index += 1;
                    let recorded = recorded.clone();
                    tokio::spawn(async move {
                        let (read, mut write) = stream.into_split();
                        let mut line = String::new();
                        let _ = BufReader::new(read).read_line(&mut line).await;
                        recorded.lock().unwrap().push(line);
                        match behavior {
                            Behavior::Reply(bytes) => {
                                let _ = write.write_all(&bytes).await;
                            }
                            Behavior::Silent => {
                                tokio::time::sleep(Duration::from_secs(60)).await;
                            }
                        }
                    });
                }
            });
            Self {
                socket,
                requests,
                _dir: dir,
            }
        }

        fn env(&self) -> Env {
            Env::from_slice(&[(SOCKET_ENV_VAR, self.socket.to_str().unwrap())])
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn rfc3339(time: SystemTime) -> String {
        DateTime::from(time).fmt(Format::DateTime).unwrap()
    }

    fn in_15_minutes() -> String {
        rfc3339(SystemTime::now() + Duration::from_secs(900))
    }

    fn success(expires_at: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"credential":"dev","expires_at":"{expires_at}","material":{{"type":"session_credentials","access_key_id":"ASIADAEMON","secret_access_key":"secret","session_token":"token","account_id":"123456789012"}}}}}}"#
        )
    }

    fn error_reply(data: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"error":{{"code":-32000,"message":"daemon says no"{data}}}}}"#
        )
    }

    fn provider(env: Env) -> CredentialDaemonProvider {
        CredentialDaemonProvider::builder()
            .configure(&ProviderConfig::empty().with_env(env))
            .credential("dev")
            .build()
    }

    fn daemon_error(err: &CredentialsError) -> &CredentialDaemonError {
        let mut source: Option<&(dyn Error + 'static)> = Some(err);
        while let Some(err) = source {
            if let Some(daemon_error) = err.downcast_ref::<CredentialDaemonError>() {
                return daemon_error;
            }
            source = err.source();
        }
        panic!("no credential daemon error in {err:?}")
    }

    fn parse_err(line: &str) -> CredentialDaemonError {
        parse_reply(line.as_bytes(), "dev").expect_err("reply should be rejected")
    }

    #[tokio::test]
    async fn success_with_all_fields() {
        let expires_at = "2099-01-02T03:04:05Z";
        let daemon = FakeDaemon::start(vec![reply_line(success(expires_at))]);
        let creds = provider(daemon.env()).provide_credentials().await.unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());
        assert_eq!("secret", creds.secret_access_key());
        assert_eq!(Some("token"), creds.session_token());
        assert_eq!(
            Some("123456789012"),
            creds.account_id().map(|id| id.as_str())
        );
        assert_eq!(
            Some(UNIX_EPOCH + Duration::from_secs(4_071_006_245)),
            creds.expiry()
        );
        assert_eq!(
            Some(&vec![AwsCredentialFeature::CredentialDaemon]),
            creds.get_property::<Vec<AwsCredentialFeature>>()
        );
        assert_eq!(
            vec!["{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"credential/get\",\"params\":{\"credential\":\"dev\"}}\n".to_string()],
            daemon.requests()
        );
    }

    #[test]
    fn success_without_optional_fields_and_with_unknown_fields() {
        let line = format!(
            r#"{{"extra":{{"a":[1,{{"b":null}}]}},"jsonrpc":"2.0","result":{{"material":{{"secret_access_key":"secret","more":[true],"type":"session_credentials","access_key_id":"AKIDAEMON"}},"credential":"dev","expires_at":"{}","later":{{}}}},"id":1,"z":"y"}}"#,
            in_15_minutes()
        );
        let creds = parse_reply(line.as_bytes(), "dev").unwrap();
        assert_eq!("AKIDAEMON", creds.access_key_id());
        assert_eq!(None, creds.session_token());
        assert_eq!(None, creds.account_id());
    }

    // Case `reply-id-and-name-not-checked`: the reply on a connection is for its one request, so
    // the reply's `id` and `result.credential` are not compared with the request's.
    #[tokio::test]
    async fn reply_id_and_name_not_checked() {
        let line = success(&in_15_minutes())
            .replace(r#""id":1"#, r#""id":999"#)
            .replace(r#""credential":"dev""#, r#""credential":"other""#);
        let daemon = FakeDaemon::start(vec![reply_line(line)]);
        let creds = provider(daemon.env()).provide_credentials().await.unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());
        assert_eq!("secret", creds.secret_access_key());
        assert_eq!(Some("token"), creds.session_token());
        assert_eq!(
            Some("123456789012"),
            creds.account_id().map(|id| id.as_str())
        );

        let ok = success(&in_15_minutes());
        for id in [r#""id":"1""#, r#""id":1.0"#, r#""id":-1"#, r#""id":null"#] {
            let line = ok.replace(r#""id":1"#, id);
            let creds = parse_reply(line.as_bytes(), "dev").unwrap();
            assert_eq!("ASIADAEMON", creds.access_key_id(), "{line}");
        }
        let line = ok.replace(r#""id":1,"#, "");
        let creds = parse_reply(line.as_bytes(), "dev").unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id(), "{line}");
    }

    #[test]
    fn request_line_escapes_and_carries_sso_params() {
        assert_eq!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"credential/get\",\"params\":{\"credential\":\"dev\",\"account_id\":\"01\\\"23\",\"role_name\":\"role\\\\name\\n\"}}\n",
            request_line("dev", Some(("01\"23", "role\\name\n")))
        );
    }

    #[tokio::test]
    async fn sso_params_are_sent() {
        let daemon = FakeDaemon::start(vec![reply_line(success(&in_15_minutes()))]);
        CredentialDaemonProvider::builder()
            .configure(&ProviderConfig::empty().with_env(daemon.env()))
            .credential("dev")
            .sso_role("0123", "role")
            .build()
            .provide_credentials()
            .await
            .unwrap();
        assert_eq!(
            vec!["{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"credential/get\",\"params\":{\"credential\":\"dev\",\"account_id\":\"0123\",\"role_name\":\"role\"}}\n".to_string()],
            daemon.requests()
        );
    }

    #[test]
    fn expires_at_formats() {
        let base = UNIX_EPOCH + Duration::from_secs(4_071_006_245);
        for (expires_at, nanos) in [
            ("2099-01-02T03:04:05Z", 0),
            ("2099-01-02T03:04:05.123Z", 123_000_000),
            ("2099-01-02T03:04:05.123456789Z", 123_456_789),
        ] {
            let creds = parse_reply(success(expires_at).as_bytes(), "dev").unwrap();
            assert_eq!(
                Some(base + Duration::from_nanos(nanos)),
                creds.expiry(),
                "{expires_at}"
            );
        }
        for expires_at in [
            "2099-01-02T03:04:05.1234567891Z",
            "2099-01-02T03:04:05.Z",
            "2099-01-02T03:04:05+01:00",
            "2099-01-02T03:04:05",
            "2099-01-02T03:04:05z",
            "2099-01-02 03:04:05Z",
            "",
        ] {
            let err = parse_err(&success(expires_at));
            assert_eq!(Code::ProtocolError, err.code(), "{expires_at}");
            assert!(err.to_string().contains("`result.expires_at`"), "{err}");
        }
        let missing = success("x").replace(r#""expires_at":"x","#, "");
        let err = parse_err(&missing);
        assert_eq!(Code::ProtocolError, err.code());
        assert!(
            err.to_string().contains("missing `result.expires_at`"),
            "{err}"
        );
    }

    #[test]
    fn error_codes() {
        const PLAIN: &str = "could not provide credential `dev`: daemon says no.";
        for (data, code, expected) in [
            (
                r#","data":{"code":"REAUTH_REQUIRED"}"#,
                Code::ReauthRequired,
                PLAIN,
            ),
            (
                r#","data":{"code":"CREDENTIAL_NOT_FOUND","x":1}"#,
                Code::CredentialNotFound,
                PLAIN,
            ),
            (
                r#","data":{"code":"REQUEST_REJECTED"}"#,
                Code::RequestRejected,
                PLAIN,
            ),
            (
                r#","data":{"code":"SOMETHING_ELSE"}"#,
                Code::RequestRejected,
                "could not provide credential `dev` (daemon code `SOMETHING_ELSE`): daemon says no.",
            ),
            (
                r#","data":"not an object""#,
                Code::RequestRejected,
                PLAIN,
            ),
            ("", Code::RequestRejected, PLAIN),
        ] {
            let err = parse_err(&error_reply(data));
            assert_eq!(code, err.code(), "{data}");
            assert!(err.to_string().contains(expected), "{err}");
        }
        // a standard JSON-RPC error without a message
        let err = parse_err(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601}}"#);
        assert_eq!(Code::RequestRejected, err.code());
        assert_eq!(
            "REQUEST_REJECTED: the credential daemon could not provide credential `dev`. See the `aws_config::credential_daemon` module docs.",
            err.to_string()
        );
    }

    #[test]
    fn unknown_domain_code() {
        let err = parse_err(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"something new","data":{"code":"FOO"}}}"#,
        );
        assert_eq!(Code::RequestRejected, err.code());
        let message = err.to_string();
        assert!(message.contains("something new"), "{message}");
        assert!(message.contains("FOO"), "{message}");
        assert_eq!(
            "REQUEST_REJECTED: the credential daemon could not provide credential `dev` (daemon code `FOO`): something new. See the `aws_config::credential_daemon` module docs.",
            message
        );
        // the daemon's code is sanitized and shortened
        let err = parse_err(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"error":{{"message":"m","data":{{"code":"F\u0007O\nO{}"}}}}}}"#,
            "X".repeat(100)
        ));
        let message = err.to_string();
        assert!(!message.chars().any(char::is_control), "{message:?}");
        assert!(
            message.contains(&format!("`FOO{}`", "X".repeat(61))),
            "{message}"
        );
        assert!(!message.contains(&"X".repeat(62)), "{message}");
    }

    #[test]
    fn error_message_is_sanitized() {
        let err = parse_err(
            r#"{"jsonrpc":"2.0","id":1,"error":{"message":"\u0007sign\n in\u009b again","data":{"code":"REAUTH_REQUIRED"}}}"#,
        );
        assert_eq!(Code::ReauthRequired, err.code());
        let message = err.to_string();
        assert!(message.contains(": sign in again."), "{message}");
        assert!(!message.chars().any(char::is_control), "{message:?}");
    }

    // A `null` `result` or `error` counts as absent.
    #[test]
    fn null_result_or_error_is_absent() {
        let err = parse_err(
            r#"{"jsonrpc":"2.0","id":1,"result":null,"error":{"message":"m","data":{"code":"CREDENTIAL_NOT_FOUND"}}}"#,
        );
        assert_eq!(Code::CredentialNotFound, err.code(), "{err}");
        assert!(err.to_string().contains("`dev`: m."), "{err}");

        let line = success(&in_15_minutes()).replace(r#""id":1,"#, r#""id":1,"error":null,"#);
        assert!(line.contains(r#""error":null"#));
        let creds = parse_reply(line.as_bytes(), "dev").unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());
        assert_eq!("secret", creds.secret_access_key());

        // `error: null` after `result`
        let ok = success(&in_15_minutes());
        let line = format!(r#"{},"error":null}}"#, &ok[..ok.len() - 1]);
        let creds = parse_reply(line.as_bytes(), "dev").unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());

        let err = parse_err(r#"{"jsonrpc":"2.0","id":1,"error":null}"#);
        assert_eq!(Code::ProtocolError, err.code());
        assert!(err.to_string().contains("neither"), "{err}");
    }

    #[test]
    fn unsupported_material() {
        let err = parse_err(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"expires_at":"{}","material":{{"type":"PLANTED_type"}}}}}}"#,
            in_15_minutes()
        ));
        assert_eq!(Code::UnsupportedMaterial, err.code());
        assert_eq!(
            "UNSUPPORTED_MATERIAL: this SDK version doesn't support material type `PLANTED_type` \
             from the credential daemon. Update the SDK. See the `aws_config::credential_daemon` module docs.",
            err.to_string()
        );
        // no other fields are needed for an unsupported type
        let err = parse_err(r#"{"jsonrpc":"2.0","id":1,"result":{"material":{"type":"x509"}}}"#);
        assert_eq!(Code::UnsupportedMaterial, err.code());
        assert!(err.to_string().contains("material type `x509`"), "{err}");
        // control characters in the type are dropped
        let err = parse_err(
            r#"{"jsonrpc":"2.0","id":1,"result":{"material":{"type":"acc\u0007ess\n_tok\u009ben\r"}}}"#,
        );
        assert_eq!(Code::UnsupportedMaterial, err.code());
        let message = err.to_string();
        assert!(
            message.contains("material type `access_token`"),
            "{message}"
        );
        assert!(message.contains("Update the SDK."), "{message}");
        assert!(!message.chars().any(char::is_control), "{message:?}");
        // a long type is shortened
        let long = "b".repeat(500);
        let err = parse_err(&format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"material":{{"type":"{long}"}}}}}}"#
        ));
        assert!(
            err.to_string()
                .contains(&format!("material type `{}`", "b".repeat(64))),
            "{err}"
        );
        assert!(!err.to_string().contains(&"b".repeat(65)), "{err}");
    }

    #[test]
    fn protocol_errors_do_not_echo_the_reply() {
        let ok = success(&in_15_minutes());
        let cases: Vec<(String, &str)> = vec![
            (
                r#"{"jsonrpc":"2.0","id":1,"result":PLANTED}"#.into(),
                "not valid JSON",
            ),
            (
                r#"{"jsonrpc":"2.0","id":1,"result":{"PLANTED":"#.into(),
                "not valid JSON",
            ),
            (r#""PLANTED""#.into(), "not a JSON object"),
            (r#"["PLANTED"]"#.into(), "not a JSON object"),
            (ok.replace(r#""2.0""#, r#""PLANTED""#), "`jsonrpc`"),
            (
                ok.replace(r#""jsonrpc":"2.0","#, r#""PLANTED":1,"#),
                "`jsonrpc`",
            ),
            (ok.replace(r#""2.0""#, r#"{"PLANTED":2}"#), "`jsonrpc`"),
            (
                ok.replace(r#""id":1,"#, r#""id":1,"error":{"message":"PLANTED"},"#),
                "both `result` and `error`",
            ),
            (r#"{"jsonrpc":"2.0","id":1,"PLANTED":{}}"#.into(), "neither"),
            (
                r#"{"jsonrpc":"2.0","id":1,"result":"PLANTED"}"#.into(),
                "`result` is not an object",
            ),
            (
                r#"{"jsonrpc":"2.0","id":1,"result":null}"#.into(),
                "neither",
            ),
            (r#"{"jsonrpc":"2.0","id":1,"error":null}"#.into(), "neither"),
            (
                r#"{"jsonrpc":"2.0","id":1,"error":["PLANTED"]}"#.into(),
                "`error` is not an object",
            ),
            (
                ok.replace(r#""material":{"#, r#""PLANTED":{"#),
                "missing `result.material`",
            ),
            (
                ok.replace(r#""type":"session_credentials","#, r#""PLANTED":1,"#),
                "missing `result.material.type`",
            ),
            (
                ok.replace(r#""access_key_id":"ASIADAEMON""#, r#""PLANTED":1"#),
                "missing `result.material.access_key_id`",
            ),
            (
                ok.replace(r#""access_key_id":"ASIADAEMON""#, r#""access_key_id":"""#),
                "missing `result.material.access_key_id`",
            ),
            (
                ok.replace(r#""secret_access_key":"secret""#, r#""PLANTED":1"#),
                "missing `result.material.secret_access_key`",
            ),
            (
                ok.replace(
                    r#""access_key_id":"ASIADAEMON""#,
                    r#""access_key_id":["PLANTED"]"#,
                ),
                "is not a string",
            ),
            (
                ok.replace(r#""expires_at":""#, r#""expires_at":"PLANTED"#),
                "`result.expires_at`",
            ),
            (
                format!(r#"{ok} {{"PLANTED":1}}"#),
                "more than one JSON value",
            ),
        ];
        for (line, reason) in cases {
            let err = parse_err(&line);
            let message = err.to_string();
            assert_eq!(Code::ProtocolError, err.code(), "{line}: {message}");
            assert!(message.contains(reason), "{line}: {message}");
            assert!(!message.contains("PLANTED"), "{line}: {message}");
            assert!(!message.contains("ASIADAEMON"), "{line}: {message}");
        }
        let err = parse_reply(b"{\"PLANTED\xff\":1}", "dev").unwrap_err();
        assert_eq!(Code::ProtocolError, err.code());
        assert!(err.to_string().contains("not valid UTF-8"), "{err}");
    }

    async fn fetch_error(behavior: Behavior) -> CredentialDaemonError {
        let daemon = FakeDaemon::start(vec![behavior]);
        let err = provider(daemon.env())
            .provide_credentials()
            .await
            .expect_err("fetch should fail");
        daemon_error(&err).clone()
    }

    #[tokio::test]
    async fn connection_closed_without_reply() {
        let err = fetch_error(Behavior::Reply(Vec::new())).await;
        assert_eq!(Code::ProtocolError, err.code());
        assert!(
            err.to_string()
                .contains("closed the connection without replying"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn reply_without_newline() {
        let err = fetch_error(Behavior::Reply(b"{\"PLANTED\":1}".to_vec())).await;
        assert_eq!(Code::ProtocolError, err.code());
        assert!(
            err.to_string().contains("reply was not newline-terminated"),
            "{err}"
        );
        assert!(!err.to_string().contains("PLANTED"), "{err}");
    }

    #[tokio::test]
    async fn bytes_after_the_newline_are_ignored() {
        let mut reply = success(&in_15_minutes()).into_bytes();
        reply.extend_from_slice(b"\nnot json at all\n");
        let daemon = FakeDaemon::start(vec![Behavior::Reply(reply)]);
        let creds = provider(daemon.env()).provide_credentials().await.unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());
    }

    /// A valid reply of exactly `len` bytes, padded with an unknown field
    fn padded_reply(len: usize) -> String {
        let ok = success(&in_15_minutes());
        let template = ok.replace(r#""credential":"dev","#, r#""pad":"","credential":"dev","#);
        let pad = len - template.len();
        let reply = template.replace(r#""pad":"""#, &format!(r#""pad":"{}""#, "x".repeat(pad)));
        assert_eq!(len, reply.len());
        reply
    }

    #[tokio::test]
    async fn reply_size_limit() {
        let daemon = FakeDaemon::start(vec![
            reply_line(padded_reply(MAX_REPLY_BYTES)),
            reply_line(padded_reply(MAX_REPLY_BYTES + 1)),
        ]);
        let provider = provider(daemon.env());
        let creds = provider.provide_credentials().await.unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());

        let err = CredentialDaemonProvider::builder()
            .configure(&ProviderConfig::empty().with_env(daemon.env()))
            .credential("dev")
            .build()
            .provide_credentials()
            .await
            .unwrap_err();
        let err = daemon_error(&err);
        assert_eq!(Code::ProtocolError, err.code());
        assert!(
            err.to_string().contains("reply exceeded 64000 bytes"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn call_timeout() {
        let daemon = FakeDaemon::start(vec![Behavior::Silent]);
        let err = CredentialDaemonProvider::builder()
            .configure(&ProviderConfig::empty().with_env(daemon.env()))
            .credential("dev")
            .call_timeout(Duration::from_millis(200))
            .build()
            .provide_credentials()
            .await
            .unwrap_err();
        let err = daemon_error(&err);
        assert_eq!(Code::NotRunning, err.code());
        assert!(
            err.to_string()
                .contains("did not reply within 200ms (call timeout)"),
            "{err}"
        );
        assert_eq!(1, daemon.requests().len());
    }

    #[tokio::test]
    async fn connect_timeout() {
        let err = connect_with_timeout(
            Path::new("/tmp/daemon.sock"),
            std::future::pending::<std::io::Result<()>>(),
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert_eq!(Code::NotRunning, err.code());
        assert!(err.to_string().contains("(connect timeout)"), "{err}");
    }

    #[tokio::test]
    async fn missing_socket_is_not_running() {
        let dir = TempDir::new();
        let path = dir.path().join("missing.sock");
        let err = call(
            &path,
            b"{}\n",
            Duration::from_secs(1),
            Duration::from_secs(5),
        )
        .await
        .unwrap_err();
        assert_eq!(Code::NotRunning, err.code());
        assert!(err.to_string().contains(path.to_str().unwrap()), "{err}");

        let err = provider(Env::from_slice(&[(SOCKET_ENV_VAR, path.to_str().unwrap())]))
            .provide_credentials()
            .await
            .unwrap_err();
        assert!(matches!(err, CredentialsError::ProviderError(_)), "{err:?}");
        assert_eq!(Code::NotRunning, daemon_error(&err).code());
    }

    #[tokio::test]
    async fn invalid_name_is_invalid_config() {
        let daemon = FakeDaemon::start(vec![reply_line(success(&in_15_minutes()))]);
        let err = CredentialDaemonProvider::builder()
            .configure(&ProviderConfig::empty().with_env(daemon.env()))
            .credential("../dev")
            .build()
            .provide_credentials()
            .await
            .unwrap_err();
        assert_eq!(Code::InvalidConfig, daemon_error(&err).code());
        assert!(daemon.requests().is_empty());
    }

    /// Binds a socket at `path`, creating its directory
    fn bind_socket(path: &Path) -> std::os::unix::net::UnixListener {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::net::UnixListener::bind(path).unwrap()
    }

    /// `$HOME/.aws/credential-daemon.sock` under `home`
    fn home_socket(home: &Path) -> PathBuf {
        home.join(".aws").join("credential-daemon.sock")
    }

    /// Makes `$HOME/.aws/credential-daemon.sock` under `home` a link to `target`
    fn link_home_socket(home: &Path, target: &Path) -> PathBuf {
        let link = home_socket(home);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, &link).unwrap();
        link
    }

    /// The env of a provider whose `HOME` is `home`, plus `vars`
    fn home_env(home: &Path, vars: &[(&str, &str)]) -> Env {
        let mut all = vec![("HOME", home.to_str().unwrap())];
        all.extend_from_slice(vars);
        Env::from_slice(&all)
    }

    async fn provider_error(env: Env) -> CredentialDaemonError {
        let err = provider(env).provide_credentials().await.unwrap_err();
        daemon_error(&err).clone()
    }

    /// Asserts `err` is the NOT_RUNNING error for no location that applies
    fn assert_no_location(err: &CredentialDaemonError) {
        assert_eq!(Code::NotRunning, err.code());
        let message = err.to_string();
        assert!(
            message.contains("`$HOME/.aws/credential-daemon.sock`"),
            "{message}"
        );
        assert!(message.contains(SOCKET_ENV_VAR), "{message}");
        assert!(
            message.contains("Install the credential daemon"),
            "{message}"
        );
        assert!(!message.contains("could not connect"), "{message}");
    }

    // Case `link`: location 2 is a link to the daemon's socket. The link's path is dialed.
    #[tokio::test]
    async fn socket_resolution_link() {
        let daemon = FakeDaemon::start(vec![reply_line(success(&in_15_minutes()))]);
        let home = TempDir::new();
        let link = link_home_socket(home.path(), &daemon.socket);
        let env = home_env(home.path(), &[]);
        assert_eq!(link, resolve_socket(&env).unwrap());
        let creds = provider(env).provide_credentials().await.unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());
        assert_eq!(1, daemon.requests().len());
    }

    // Case `link-is-a-socket` (`home-socket`): the daemon listens at location 2 itself.
    #[tokio::test]
    async fn socket_resolution_home_socket() {
        let home = TempDir::new();
        let socket = home_socket(home.path());
        let _listener = bind_socket(&socket);
        assert_eq!(socket, resolve_socket(&home_env(home.path(), &[])).unwrap());
    }

    // A regular file at location 2 still applies, and the dial fails with no fall-through.
    #[tokio::test]
    async fn socket_resolution_home_regular_file() {
        let home = TempDir::new();
        let path = home_socket(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not a socket").unwrap();
        let env = home_env(home.path(), &[]);
        assert_eq!(path, resolve_socket(&env).unwrap());
        let err = provider_error(env).await;
        assert_eq!(Code::NotRunning, err.code());
        assert!(
            err.to_string().contains(&format!("`{}`", path.display())),
            "{err}"
        );
    }

    // Case `not-running` (`link-stale`): location 2 links to a socket nobody listens on. The
    // dial fails, the error names the link and not its target, and nothing else is tried.
    #[tokio::test]
    async fn socket_resolution_link_stale() {
        let daemon_dir = TempDir::new();
        let stale = daemon_dir.path().join("stale.sock");
        drop(bind_socket(&stale));
        assert!(stale.exists());
        let home = TempDir::new();
        let link = link_home_socket(home.path(), &stale);
        let err = provider_error(home_env(home.path(), &[])).await;
        assert_eq!(Code::NotRunning, err.code());
        let message = err.to_string();
        assert!(
            message.contains(&format!(
                "could not connect to the credential daemon at `{}`",
                link.display()
            )),
            "{message}"
        );
        assert!(!message.contains(stale.to_str().unwrap()), "{message}");
    }

    // Case `link-dangling`: location 2 is a link whose target is missing. The error names the
    // link and its target, and nothing is dialed.
    #[tokio::test]
    async fn socket_resolution_link_dangling() {
        let daemon_dir = TempDir::new();
        let missing = daemon_dir.path().join("missing-daemon.sock");
        let home = TempDir::new();
        let link = link_home_socket(home.path(), &missing);
        let env = home_env(home.path(), &[]);
        let err = resolve_socket(&env).unwrap_err();
        assert_eq!(Code::NotRunning, err.code());
        let err = provider_error(env).await;
        assert_eq!(Code::NotRunning, err.code());
        let message = err.to_string();
        assert!(
            message.contains(&format!("`{}`", link.display())),
            "{message}"
        );
        assert!(
            message.contains(&format!("`{}`", missing.display())),
            "{message}"
        );
        assert!(!message.contains("could not connect"), "{message}");
    }

    // Case `no-link`: nothing at location 2 and no env var, so nothing is dialed.
    #[tokio::test]
    async fn socket_resolution_no_link() {
        let home = TempDir::new();
        let env = home_env(home.path(), &[]);
        assert_no_location(&resolve_socket(&env).unwrap_err());
        assert_no_location(&provider_error(env).await);
        // `$HOME/.aws` is there, but the socket isn't
        std::fs::create_dir_all(home.path().join(".aws")).unwrap();
        assert_no_location(&resolve_socket(&home_env(home.path(), &[])).unwrap_err());
        // `$HOME/.aws` is a file
        let file_home = TempDir::new();
        std::fs::write(file_home.path().join(".aws"), "").unwrap();
        assert_no_location(&resolve_socket(&home_env(file_home.path(), &[])).unwrap_err());
    }

    // Cases `no-link-home-unset` and `no-link-home-relative`: location 2 is skipped when
    // `HOME` is unset, empty or relative, even with a socket under the relative path.
    #[tokio::test]
    async fn socket_resolution_no_link_home_unset_or_relative() {
        // a socket under a relative HOME, relative to the working directory of the test
        let cwd = std::env::current_dir().unwrap();
        let relative = PathBuf::from(format!("cd-test-home-{}", std::process::id()));
        let _relative_home = TempDir(cwd.join(&relative));
        // bound by its relative path, which keeps it short
        let _listener = bind_socket(&home_socket(&relative));
        let relative_home = relative.to_str().unwrap();
        for vars in [
            vec![],
            vec![("HOME", "")],
            vec![("HOME", "relative/home")],
            vec![("HOME", relative_home)],
        ] {
            assert_no_location(&resolve_socket(&Env::from_slice(&vars)).unwrap_err());
            assert_no_location(&provider_error(Env::from_slice(&vars)).await);
        }
    }

    // Case `env`: an absolute env var wins over location 2, whether or not anything is there.
    #[tokio::test]
    async fn socket_resolution_env_var() {
        let daemon = FakeDaemon::start(vec![reply_line(success(&in_15_minutes()))]);
        let home = TempDir::new();
        let link = link_home_socket(home.path(), &daemon.socket);
        assert_eq!(
            PathBuf::from("/somewhere/daemon.sock"),
            resolve_socket(&home_env(
                home.path(),
                &[(SOCKET_ENV_VAR, "/somewhere/daemon.sock")]
            ))
            .unwrap()
        );
        // a relative or empty env var counts as unset
        for value in ["relative/daemon.sock", ""] {
            assert_eq!(
                link,
                resolve_socket(&home_env(home.path(), &[(SOCKET_ENV_VAR, value)])).unwrap()
            );
            assert_no_location(
                &resolve_socket(&Env::from_slice(&[(SOCKET_ENV_VAR, value)])).unwrap_err(),
            );
        }
    }

    // Case `env-file`: the env var names a file that isn't a socket. It is dialed, the error
    // names it, and location 2 is not tried.
    #[tokio::test]
    async fn socket_resolution_env_file() {
        let daemon = FakeDaemon::start(vec![reply_line(success(&in_15_minutes()))]);
        let home = TempDir::new();
        link_home_socket(home.path(), &daemon.socket);
        let file = home.path().join("env-file.sock");
        std::fs::write(&file, "not a socket").unwrap();
        let err = provider_error(home_env(
            home.path(),
            &[(SOCKET_ENV_VAR, file.to_str().unwrap())],
        ))
        .await;
        assert_eq!(Code::NotRunning, err.code());
        assert!(
            err.to_string().contains(&format!("`{}`", file.display())),
            "{err}"
        );
        assert!(daemon.requests().is_empty());
    }

    #[tokio::test]
    async fn socket_is_resolved_on_every_fetch() {
        let first = FakeDaemon::start(vec![reply_line(success(&in_15_minutes()))]);
        let second = FakeDaemon::start(vec![reply_line(success(&in_15_minutes()))]);
        let env = Env::from_slice(&[(SOCKET_ENV_VAR, first.socket.to_str().unwrap())]);
        let provider = provider(env);
        provider.provide_credentials().await.unwrap();
        // replace the first daemon's socket with a link to the second one
        std::fs::remove_file(&first.socket).unwrap();
        std::os::unix::fs::symlink(&second.socket, &first.socket).unwrap();
        provider.provide_credentials().await.unwrap();
        assert_eq!(1, first.requests().len());
        assert_eq!(1, second.requests().len());
    }

    fn refresh_provider(
        daemon: &FakeDaemon,
        time_source: &ManualTimeSource,
    ) -> CredentialDaemonProvider {
        CredentialDaemonProvider::builder()
            .configure(
                &ProviderConfig::empty()
                    .with_env(daemon.env())
                    .with_time_source(time_source.clone()),
            )
            .credential("dev")
            .build()
    }

    /// Expected failures: the provider has no static stability of its own, and the identity
    /// cache gets it with the Credential Refresh SEP.
    const NEEDS_REFRESH_SEP: &str = "needs the Credential Refresh SEP";

    #[tokio::test]
    #[should_panic(expected = "needs the Credential Refresh SEP")]
    async fn recoverable_refresh_error_returns_unexpired_credentials() {
        let now = SystemTime::now();
        let time_source = ManualTimeSource::new(now);
        let daemon = FakeDaemon::start(vec![
            reply_line(success(&rfc3339(now + Duration::from_secs(900)))),
            reply_line("not json"),
        ]);
        let provider = refresh_provider(&daemon, &time_source);
        let first: Credentials = provider.provide_credentials().await.unwrap();
        let next = provider
            .provide_credentials()
            .await
            .expect(NEEDS_REFRESH_SEP);
        assert_eq!(first, next);
    }

    #[tokio::test]
    #[should_panic(expected = "needs the Credential Refresh SEP")]
    async fn not_running_refresh_error_returns_unexpired_credentials() {
        let now = SystemTime::now();
        let time_source = ManualTimeSource::new(now);
        let daemon = FakeDaemon::start(vec![reply_line(success(&rfc3339(
            now + Duration::from_secs(900),
        )))]);
        let provider = refresh_provider(&daemon, &time_source);
        let first = provider.provide_credentials().await.unwrap();
        std::fs::remove_file(&daemon.socket).unwrap();
        let next = provider
            .provide_credentials()
            .await
            .expect(NEEDS_REFRESH_SEP);
        assert_eq!(first, next);
    }

    #[tokio::test]
    async fn non_recoverable_refresh_error_is_returned() {
        let now = SystemTime::now();
        let time_source = ManualTimeSource::new(now);
        let daemon = FakeDaemon::start(vec![
            reply_line(success(&rfc3339(now + Duration::from_secs(900)))),
            reply_line(error_reply(r#","data":{"code":"REAUTH_REQUIRED"}"#)),
            reply_line("not json"),
        ]);
        let provider = refresh_provider(&daemon, &time_source);
        provider.provide_credentials().await.unwrap();
        let err = provider.provide_credentials().await.unwrap_err();
        assert_eq!(Code::ReauthRequired, daemon_error(&err).code());
        // the next error is returned too
        let err = provider.provide_credentials().await.unwrap_err();
        assert_eq!(Code::ProtocolError, daemon_error(&err).code());
    }

    // expiry-past: a requiresCredentialRefresh case that passes on the existing refresh code
    #[tokio::test]
    async fn expiry_past_is_returned_as_received() {
        let expires_at = SystemTime::now() - Duration::from_secs(3600);
        let daemon = FakeDaemon::start(vec![reply_line(success(&rfc3339(expires_at)))]);
        let creds = provider(daemon.env()).provide_credentials().await.unwrap();
        assert_eq!("ASIADAEMON", creds.access_key_id());
        assert_eq!(
            DateTime::from(expires_at).secs(),
            DateTime::from(creds.expiry().unwrap()).secs()
        );
    }

    /// A credential daemon provider behind the SDK's default identity cache
    /// (`IdentityCache::lazy()`), with a manual clock.
    struct Cached {
        cache: SharedIdentityCache,
        resolver: SharedIdentityResolver,
        components: RuntimeComponents,
    }

    impl Cached {
        fn new(daemon: &FakeDaemon) -> Self {
            let time_source = ManualTimeSource::new(SystemTime::now());
            let provider = CredentialDaemonProvider::builder()
                .configure(
                    &ProviderConfig::empty()
                        .with_env(daemon.env())
                        .with_time_source(time_source.clone()),
                )
                .credential("dev")
                .build();
            let components = RuntimeComponentsBuilder::for_tests()
                .with_time_source(Some(time_source))
                .with_sleep_impl(Some(TokioSleep::new()))
                .build()
                .unwrap();
            Self {
                cache: IdentityCache::lazy().build(),
                resolver: SharedIdentityResolver::new(SharedCredentialsProvider::new(provider)),
                components,
            }
        }

        async fn resolve(&self) -> Result<Credentials, BoxError> {
            let config_bag = ConfigBag::base();
            let identity = self
                .cache
                .resolve_cached_identity(self.resolver.clone(), &self.components, &config_bag)
                .await?;
            Ok(identity.data::<Credentials>().unwrap().clone())
        }
    }

    fn success_with_key(access_key_id: &str, expires_at: &str) -> String {
        success(expires_at).replace("ASIADAEMON", access_key_id)
    }

    // invalidated-refreshes
    #[tokio::test]
    #[should_panic(expected = "needs the Credential Refresh SEP")]
    async fn invalidated_refreshes() {
        let daemon = FakeDaemon::start(vec![
            reply_line(success_with_key("ASIAREJECTED", &in_15_minutes())),
            reply_line(success(&in_15_minutes())),
        ]);
        let cached = Cached::new(&daemon);
        assert_eq!(
            "ASIAREJECTED",
            cached.resolve().await.unwrap().access_key_id()
        );
        // smithy-rs has no `Invalidate()`, so the cache returns the rejected credentials again
        let next = cached.resolve().await.unwrap();
        assert_eq!(2, daemon.requests().len(), "{NEEDS_REFRESH_SEP}");
        assert_eq!("ASIADAEMON", next.access_key_id(), "{NEEDS_REFRESH_SEP}");
    }

    // invalidated-same-key
    #[tokio::test]
    #[should_panic(expected = "needs the Credential Refresh SEP")]
    async fn invalidated_same_key() {
        let daemon = FakeDaemon::start(vec![reply_line(success_with_key(
            "ASIAREJECTED",
            &in_15_minutes(),
        ))]);
        let cached = Cached::new(&daemon);
        assert_eq!(
            "ASIAREJECTED",
            cached.resolve().await.unwrap().access_key_id()
        );
        // smithy-rs has no `Invalidate()`, so the cache returns the rejected credentials again
        let next = cached.resolve().await;
        assert_eq!(2, daemon.requests().len(), "{NEEDS_REFRESH_SEP}");
        assert!(next.is_err(), "{NEEDS_REFRESH_SEP}");
    }
}
