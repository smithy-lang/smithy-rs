/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use aws_smithy_http::middleware::MapRequest;
use aws_smithy_http::operation::Request;
use aws_types::build_metadata::{OsFamily, BUILD_METADATA};
use aws_types::os_shim_internal::Env;
use http::header::{HeaderName, InvalidHeaderValue, USER_AGENT};
use http::HeaderValue;
use std::borrow::Cow;
use std::convert::TryFrom;
use std::fmt;
use std::fmt::{Display, Formatter};
use thiserror::Error;

/// AWS User Agent
///
/// Ths struct should be inserted into the [`PropertyBag`](aws_smithy_http::operation::Request::properties)
/// during operation construction. [`UserAgentStage`](UserAgentStage) reads `AwsUserAgent`
/// from the property bag and sets the `User-Agent` and `x-amz-user-agent` headers.
#[derive(Clone)]
pub struct AwsUserAgent {
    sdk_metadata: SdkMetadata,
    api_metadata: ApiMetadata,
    os_metadata: OsMetadata,
    language_metadata: LanguageMetadata,
    exec_env_metadata: Option<ExecEnvMetadata>,
    feature_metadata: Vec<FeatureMetadata>,
    config_metadata: Vec<ConfigMetadata>,
    framework_metadata: Vec<FrameworkMetadata>,
    app_id: Option<Cow<'static, str>>,
}

impl AwsUserAgent {
    /// Load a User Agent configuration from the environment
    ///
    /// This utilizes [`BUILD_METADATA`](const@aws_types::build_metadata::BUILD_METADATA) from `aws_types`
    /// to capture the Rust version & target platform. `ApiMetadata` provides
    /// the version & name of the specific service.
    pub fn new_from_environment(
        env: Env,
        api_metadata: ApiMetadata,
        feature_metadata: Vec<FeatureMetadata>,
        config_metadata: Vec<ConfigMetadata>,
        framework_metadata: Vec<FrameworkMetadata>,
        app_id: Option<Cow<'static, str>>,
    ) -> Self {
        let build_metadata = &BUILD_METADATA;
        let sdk_metadata = SdkMetadata {
            name: "rust",
            version: build_metadata.core_pkg_version,
        };
        let os_metadata = OsMetadata {
            os_family: &build_metadata.os_family,
            version: None,
        };
        let mut exec_env_metadata = None;
        if let Ok(exec_env) = env.get("AWS_EXECUTION_ENV") {
            exec_env_metadata = Some(ExecEnvMetadata { name: exec_env });
        }
        AwsUserAgent {
            sdk_metadata,
            api_metadata,
            os_metadata,
            language_metadata: LanguageMetadata {
                lang: "rust",
                version: BUILD_METADATA.rust_version,
                extras: Default::default(),
            },
            exec_env_metadata,
            feature_metadata,
            config_metadata,
            framework_metadata,
            app_id,
        }
    }

    /// For test purposes, construct an environment-independent User Agent
    ///
    /// Without this, running CI on a different platform would produce different user agent strings
    pub fn for_tests() -> Self {
        Self {
            sdk_metadata: SdkMetadata {
                name: "rust",
                version: "0.123.test",
            },
            api_metadata: ApiMetadata {
                service_id: "test-service".into(),
                version: "0.123",
            },
            os_metadata: OsMetadata {
                os_family: &OsFamily::Windows,
                version: Some("XPSP3".to_string()),
            },
            language_metadata: LanguageMetadata {
                lang: "rust",
                version: "1.50.0",
                extras: Default::default(),
            },
            exec_env_metadata: None,
            feature_metadata: Vec::new(),
            config_metadata: Vec::new(),
            framework_metadata: Vec::new(),
            app_id: None,
        }
    }

    /// Generate a new-style user agent style header
    ///
    /// This header should be set at `x-amz-user-agent`
    pub fn aws_ua_header(&self) -> String {
        /*
        ABNF for the user agent (see the bottom of the file for complete ABNF):
        ua-string = sdk-metadata RWS
                    [api-metadata RWS]
                    os-metadata RWS
                    language-metadata RWS
                    [env-metadata RWS]
                    *(feat-metadata RWS)
                    *(config-metadata RWS)
                    *(framework-metadata RWS)
                    [appId]
        */
        let mut ua_value = String::new();
        use std::fmt::Write;
        // unwrap calls should never fail because string formatting will always succeed.
        write!(ua_value, "{} ", &self.sdk_metadata).unwrap();
        write!(ua_value, "{} ", &self.api_metadata).unwrap();
        write!(ua_value, "{} ", &self.os_metadata).unwrap();
        write!(ua_value, "{} ", &self.language_metadata).unwrap();
        if let Some(ref env_meta) = self.exec_env_metadata {
            write!(ua_value, "{} ", env_meta).unwrap();
        }
        for feature in &self.feature_metadata {
            write!(ua_value, "{} ", feature).unwrap();
        }
        for config in &self.config_metadata {
            write!(ua_value, "{} ", config).unwrap();
        }
        for framework in &self.framework_metadata {
            write!(ua_value, "{} ", framework).unwrap();
        }
        if let Some(app_id) = &self.app_id {
            write!(ua_value, "app/{}", app_id).unwrap();
        }
        if ua_value.ends_with(' ') {
            ua_value.truncate(ua_value.len() - 1);
        }
        ua_value
    }

    /// Generate an old-style User-Agent header for backward compatibility
    ///
    /// This header is intended to be set at `User-Agent`
    pub fn ua_header(&self) -> String {
        let mut ua_value = String::new();
        use std::fmt::Write;
        write!(ua_value, "{} ", &self.sdk_metadata).unwrap();
        write!(ua_value, "{} ", &self.os_metadata).unwrap();
        write!(ua_value, "{}", &self.language_metadata).unwrap();
        ua_value
    }
}

#[derive(Clone, Copy)]
pub struct SdkMetadata {
    name: &'static str,
    version: &'static str,
}

impl Display for SdkMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "aws-sdk-{}/{}", self.name, self.version)
    }
}

#[derive(Clone)]
pub struct ApiMetadata {
    service_id: Cow<'static, str>,
    version: &'static str,
}

impl ApiMetadata {
    pub const fn new(service_id: &'static str, version: &'static str) -> Self {
        Self {
            service_id: Cow::Borrowed(service_id),
            version,
        }
    }
}

impl Display for ApiMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "api/{}/{}", self.service_id, self.version)
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AdditionalMetadata {
    value: Cow<'static, str>,
}

impl AdditionalMetadata {
    pub const fn new_static(value: &'static str) -> Self {
        Self {
            value: Cow::Borrowed(value),
        }
    }

    pub fn new(value: impl Into<Cow<'static, str>>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

impl Display for AdditionalMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // additional-metadata = "md/" ua-pair
        write!(f, "md/{}", self.value)
    }
}

#[derive(Clone, Debug, Default)]
struct AdditionalMetadataList(Vec<AdditionalMetadata>);

impl AdditionalMetadataList {
    pub const fn new() -> AdditionalMetadataList {
        AdditionalMetadataList(Vec::new())
    }

    fn push(&mut self, metadata: AdditionalMetadata) {
        self.0.push(metadata);
    }
}

impl Display for AdditionalMetadataList {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for metadata in &self.0 {
            write!(f, " {}", metadata)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct FeatureMetadata {
    name: Cow<'static, str>,
    version: Option<Cow<'static, str>>,
    additional: AdditionalMetadataList,
}

impl FeatureMetadata {
    pub const fn new_static(name: &'static str, version: Option<Cow<'static, str>>) -> Self {
        Self {
            name: Cow::Borrowed(name),
            version,
            additional: AdditionalMetadataList::new(),
        }
    }

    pub fn new(name: impl Into<Cow<'static, str>>, version: Option<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            version,
            additional: Default::default(),
        }
    }

    pub fn with_additional(mut self, metadata: AdditionalMetadata) -> Self {
        self.additional.push(metadata);
        self
    }
}

impl Display for FeatureMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // feat-metadata = "ft/" name ["/" version] *(RWS additional-metadata)
        if let Some(version) = &self.version {
            write!(f, "ft/{}/{}{}", self.name, version, self.additional)
        } else {
            write!(f, "ft/{}{}", self.name, self.additional)
        }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ConfigMetadata {
    config: Cow<'static, str>,
    value: Option<Cow<'static, str>>,
}

impl ConfigMetadata {
    pub const fn new_static(config: &'static str, value: Option<Cow<'static, str>>) -> Self {
        Self {
            config: Cow::Borrowed(config),
            value,
        }
    }

    pub fn new(config: impl Into<Cow<'static, str>>, value: Option<Cow<'static, str>>) -> Self {
        Self {
            config: config.into(),
            value,
        }
    }
}

impl Display for ConfigMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // config-metadata = "cfg/" config ["/" name]
        if let Some(value) = &self.value {
            write!(f, "cfg/{}/{}", self.config, value)
        } else {
            write!(f, "cfg/{}", self.config)
        }
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct FrameworkMetadata {
    name: Cow<'static, str>,
    version: Option<Cow<'static, str>>,
    additional: AdditionalMetadataList,
}

impl FrameworkMetadata {
    pub const fn new_static(name: &'static str, version: Option<Cow<'static, str>>) -> Self {
        Self {
            name: Cow::Borrowed(name),
            version,
            additional: AdditionalMetadataList::new(),
        }
    }

    pub fn new(name: impl Into<Cow<'static, str>>, version: Option<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            version,
            additional: Default::default(),
        }
    }

    pub fn with_additional(mut self, metadata: AdditionalMetadata) -> Self {
        self.additional.push(metadata);
        self
    }
}

impl Display for FrameworkMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // framework-metadata = "lib/" name ["/" version] *(RWS additional-metadata)
        if let Some(version) = &self.version {
            write!(f, "lib/{}/{}{}", self.name, version, self.additional)
        } else {
            write!(f, "lib/{}{}", self.name, self.additional)
        }
    }
}

#[derive(Clone)]
struct OsMetadata {
    os_family: &'static OsFamily,
    version: Option<String>,
}

impl Display for OsMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let os_family = match self.os_family {
            OsFamily::Windows => "windows",
            OsFamily::Linux => "linux",
            OsFamily::Macos => "macos",
            OsFamily::Android => "android",
            OsFamily::Ios => "ios",
            OsFamily::Other => "other",
        };
        write!(f, "os/{}", os_family)?;
        if let Some(ref version) = self.version {
            write!(f, "/{}", version)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct LanguageMetadata {
    lang: &'static str,
    version: &'static str,
    extras: AdditionalMetadataList,
}
impl Display for LanguageMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // language-metadata = "lang/" language "/" version *(RWS additional-metadata)
        write!(f, "lang/{}/{}{}", self.lang, self.version, self.extras)
    }
}

#[derive(Clone)]
struct ExecEnvMetadata {
    name: String,
}
impl Display for ExecEnvMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "exec-env/{}", &self.name)
    }
}

#[non_exhaustive]
#[derive(Default, Clone, Debug)]
pub struct UserAgentStage;

impl UserAgentStage {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Error)]
pub enum UserAgentStageError {
    #[error("User agent missing from property bag")]
    UserAgentMissing,
    #[error("Provided user agent header was invalid")]
    InvalidHeader(#[from] InvalidHeaderValue),
}

lazy_static::lazy_static! {
    static ref X_AMZ_USER_AGENT: HeaderName = HeaderName::from_static("x-amz-user-agent");
}

impl MapRequest for UserAgentStage {
    type Error = UserAgentStageError;

    fn apply(&self, request: Request) -> Result<Request, Self::Error> {
        request.augment(|mut req, conf| {
            let ua = conf
                .get::<AwsUserAgent>()
                .ok_or(UserAgentStageError::UserAgentMissing)?;
            req.headers_mut()
                .append(USER_AGENT, HeaderValue::try_from(ua.ua_header())?);
            req.headers_mut().append(
                X_AMZ_USER_AGENT.clone(),
                HeaderValue::try_from(ua.aws_ua_header())?,
            );

            Ok(req)
        })
    }
}

#[cfg(test)]
mod test {
    use crate::user_agent::{
        AdditionalMetadata, ApiMetadata, AwsUserAgent, ConfigMetadata, FrameworkMetadata,
        UserAgentStage,
    };
    use crate::user_agent::{FeatureMetadata, X_AMZ_USER_AGENT};
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_http::middleware::MapRequest;
    use aws_smithy_http::operation;
    use aws_types::build_metadata::OsFamily;
    use aws_types::os_shim_internal::Env;
    use http::header::USER_AGENT;
    use std::borrow::Cow;

    fn make_deterministic(ua: &mut AwsUserAgent) {
        // hard code some variable things for a deterministic test
        ua.sdk_metadata.version = "0.1";
        ua.language_metadata.version = "1.50.0";
        ua.os_metadata.os_family = &OsFamily::Macos;
        ua.os_metadata.version = Some("1.15".to_string());
    }

    #[test]
    fn generate_a_valid_ua() {
        let api_metadata = ApiMetadata {
            service_id: "dynamodb".into(),
            version: "123",
        };
        let mut ua = AwsUserAgent::new_from_environment(
            Env::from_slice(&[]),
            api_metadata,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        );
        make_deterministic(&mut ua);
        assert_eq!(
            ua.aws_ua_header(),
            "aws-sdk-rust/0.1 api/dynamodb/123 os/macos/1.15 lang/rust/1.50.0"
        );
        assert_eq!(
            ua.ua_header(),
            "aws-sdk-rust/0.1 os/macos/1.15 lang/rust/1.50.0"
        );
    }

    #[test]
    fn generate_a_valid_ua_with_execution_env() {
        let api_metadata = ApiMetadata {
            service_id: "dynamodb".into(),
            version: "123",
        };
        let mut ua = AwsUserAgent::new_from_environment(
            Env::from_slice(&[("AWS_EXECUTION_ENV", "lambda")]),
            api_metadata,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
        );
        make_deterministic(&mut ua);
        assert_eq!(
            ua.aws_ua_header(),
            "aws-sdk-rust/0.1 api/dynamodb/123 os/macos/1.15 lang/rust/1.50.0 exec-env/lambda"
        );
        assert_eq!(
            ua.ua_header(),
            "aws-sdk-rust/0.1 os/macos/1.15 lang/rust/1.50.0"
        );
    }

    #[test]
    fn generate_a_valid_ua_with_features() {
        let api_metadata = ApiMetadata {
            service_id: "dynamodb".into(),
            version: "123",
        };
        let mut ua = AwsUserAgent::new_from_environment(
            Env::from_slice(&[]),
            api_metadata,
            vec![
                FeatureMetadata::new_static("test-feature", Some(Cow::Borrowed("1.0"))),
                FeatureMetadata::new_static("other-feature", None)
                    .with_additional(AdditionalMetadata::new_static("asdf")),
            ],
            vec![],
            vec![],
            None,
        );
        make_deterministic(&mut ua);
        assert_eq!(
            ua.aws_ua_header(),
            "aws-sdk-rust/0.1 api/dynamodb/123 os/macos/1.15 lang/rust/1.50.0 ft/test-feature/1.0 ft/other-feature md/asdf"
        );
        assert_eq!(
            ua.ua_header(),
            "aws-sdk-rust/0.1 os/macos/1.15 lang/rust/1.50.0"
        );
    }

    #[test]
    fn generate_a_valid_ua_with_config() {
        let api_metadata = ApiMetadata {
            service_id: "dynamodb".into(),
            version: "123",
        };
        let mut ua = AwsUserAgent::new_from_environment(
            Env::from_slice(&[]),
            api_metadata,
            vec![],
            vec![
                ConfigMetadata::new_static("some-config", Some(Cow::Borrowed("5"))),
                ConfigMetadata::new_static("other-config", None),
            ],
            vec![],
            None,
        );
        make_deterministic(&mut ua);
        assert_eq!(
            ua.aws_ua_header(),
            "aws-sdk-rust/0.1 api/dynamodb/123 os/macos/1.15 lang/rust/1.50.0 cfg/some-config/5 cfg/other-config"
        );
        assert_eq!(
            ua.ua_header(),
            "aws-sdk-rust/0.1 os/macos/1.15 lang/rust/1.50.0"
        );
    }

    #[test]
    fn generate_a_valid_ua_with_frameworks() {
        let api_metadata = ApiMetadata {
            service_id: "dynamodb".into(),
            version: "123",
        };
        let mut ua = AwsUserAgent::new_from_environment(
            Env::from_slice(&[]),
            api_metadata,
            vec![],
            vec![],
            vec![
                FrameworkMetadata::new_static("some-framework", Some(Cow::Borrowed("1.3")))
                    .with_additional(AdditionalMetadata::new_static("something")),
                FrameworkMetadata::new_static("other", None),
            ],
            None,
        );
        make_deterministic(&mut ua);
        assert_eq!(
            ua.aws_ua_header(),
            "aws-sdk-rust/0.1 api/dynamodb/123 os/macos/1.15 lang/rust/1.50.0 lib/some-framework/1.3 md/something lib/other"
        );
        assert_eq!(
            ua.ua_header(),
            "aws-sdk-rust/0.1 os/macos/1.15 lang/rust/1.50.0"
        );
    }

    #[test]
    fn generate_a_valid_ua_with_app_id() {
        let api_metadata = ApiMetadata {
            service_id: "dynamodb".into(),
            version: "123",
        };
        let mut ua = AwsUserAgent::new_from_environment(
            Env::from_slice(&[]),
            api_metadata,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Some(Cow::Borrowed("my_app")),
        );
        make_deterministic(&mut ua);
        assert_eq!(
            ua.aws_ua_header(),
            "aws-sdk-rust/0.1 api/dynamodb/123 os/macos/1.15 lang/rust/1.50.0 app/my_app"
        );
        assert_eq!(
            ua.ua_header(),
            "aws-sdk-rust/0.1 os/macos/1.15 lang/rust/1.50.0"
        );
    }

    #[test]
    fn ua_stage_adds_headers() {
        let stage = UserAgentStage::new();
        let req = operation::Request::new(http::Request::new(SdkBody::from("some body")));
        stage
            .apply(req)
            .expect_err("adding UA should fail without a UA set");
        let mut req = operation::Request::new(http::Request::new(SdkBody::from("some body")));
        req.properties_mut()
            .insert(AwsUserAgent::new_from_environment(
                Env::from_slice(&[]),
                ApiMetadata {
                    service_id: "dynamodb".into(),
                    version: "0.123",
                },
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
            ));
        let req = stage.apply(req).expect("setting user agent should succeed");
        let (req, _) = req.into_parts();
        req.headers()
            .get(USER_AGENT)
            .expect("UA header should be set");
        req.headers()
            .get(&*X_AMZ_USER_AGENT)
            .expect("UA header should be set");
    }
}

/*
Appendix: User Agent ABNF
sdk-ua-header        = "x-amz-user-agent:" OWS ua-string OWS
ua-pair              = ua-name ["/" ua-value]
ua-name              = token
ua-value             = token
version              = token
name                 = token
service-id           = token
sdk-name             = java / ruby / php / dotnet / python / cli / kotlin / rust / js / cpp / go / go-v2
os-family            = windows / linux / macos / android / ios / other
config               = retry-mode
additional-metadata  = "md/" ua-pair
sdk-metadata         = "aws-sdk-" sdk-name "/" version
api-metadata         = "api/" service-id "/" version
os-metadata          = "os/" os-family ["/" version]
language-metadata    = "lang/" language "/" version *(RWS additional-metadata)
env-metadata         = "exec-env/" name
feat-metadata        = "ft/" name ["/" version] *(RWS additional-metadata)
config-metadata      = "cfg/" config ["/" name]
framework-metadata   = "lib/" name ["/" version] *(RWS additional-metadata)
appId                = "app/" name
ua-string            = sdk-metadata RWS
                       [api-metadata RWS]
                       os-metadata RWS
                       language-metadata RWS
                       [env-metadata RWS]
                       *(feat-metadata RWS)
                       *(config-metadata RWS)
                       *(framework-metadata RWS)
                       [appId]

# New metadata field might be added in the future and they must follow this format
prefix               = token
metadata             = prefix "/" ua-pair

# token, RWS and OWS are defined in [RFC 7230](https://tools.ietf.org/html/rfc7230)
OWS            = *( SP / HTAB )
               ; optional whitespace
RWS            = 1*( SP / HTAB )
               ; required whitespace
token          = 1*tchar
tchar          = "!" / "#" / "$" / "%" / "&" / "'" / "*" / "+" / "-" / "." /
                 "^" / "_" / "`" / "|" / "~" / DIGIT / ALPHA
*/
