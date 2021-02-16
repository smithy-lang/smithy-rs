/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fmt;

#[derive(Default)]
struct AwsUserAgent {
    sdk_metadata: Option<SdkMetadata>,
    api_metadata: Option<ApiMetadata>,
    os_metadata: Option<OsMetadata>,
    language_metadata: Option<LanguageMetadata>,
    exec_env_metadata: Option<ExecEnvMetadata>
}


pub type UaCreationError = Box<dyn Error>;

impl AwsUserAgent {
    fn aws_ua_header(&self) -> Result<String, UaCreationError>{
        /** ua-string            = sdk-metadata RWS
                       [api-metadata RWS]
                       os-metadata RWS
                       language-metadata RWS
                       [env-metadata RWS]
                       *(feat-metadata RWS)
                       *(config-metadata RWS)
                       *(framework-metadata RWS)
                       [appId] */
        let mut ua_value = String::new();
        use std::fmt::Write;
        write!(ua_value, "{}", &self.sdk_metadata.as_ref().ok_or("Missing SDK Metadata")?);
        Ok(ua_value)
    }
}

struct SdkMetadata { name: &'static str, version: &'static str }
impl SdkMetadata {
    pub fn new(version: &'static str) -> Self {
        SdkMetadata { name: "rust", version }
    }
}
impl Display for SdkMetadata {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "aws-sdk-{}/{}", self.name, self.version)
    }
}
struct ApiMetadata { service_id: String, version: &'static str }
struct AdditionalMetadata { key: String, value: String }
struct OsMetadata { os_family: OsFamily, version: Option<String> }
struct LanguageMetadata { lang: &'static str, version: &'static str, extras: Vec<AdditionalMetadata> }
struct ExecEnvMetadata { name: String }

enum OsFamily {
    Windows,
    Linux,
    Macos,
    Android,
    Ios,
    Other
}

struct UaValue(String);

#[cfg(test)]
mod test {
    use crate::user_agent::{AwsUserAgent, SdkMetadata};

    #[test]
    fn generate_a_valid_ua() {
        let mut ua = AwsUserAgent::default();
        ua.aws_ua_header().expect_err("sdk not defined");
        ua.sdk_metadata = Some(SdkMetadata::new("0.1"));
        assert_eq!(ua.aws_ua_header().unwrap(), "aws-sdk-rust/0.1");
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
config-metadata      = "cfg/" config "/" name
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
