use crate::connector::must_have_connector;
use aws_types::config::Config;
use aws_types::credential::ProvideCredentials;
use aws_types::region::Region;
use smithy_client::DynConnector;
use std::sync::Arc;

pub mod environment;
#[cfg(feature = "meta")]
pub mod meta;
#[cfg(feature = "profile")]
pub mod profile;

#[cfg(feature = "default-provider")]
pub mod default_provider;

mod test_case;

#[cfg(feature = "sts")]
pub mod sts;

#[cfg(feature = "web-identity-token")]
pub mod web_identity_token;

#[cfg(feature = "default-provider")]
pub async fn load_config_from_environment() -> aws_types::config::Config {
    default_provider::env_loader().load().await
}

#[cfg(feature = "default-provider")]
pub use default_provider::env_loader;

mod connector {

    // create a default connector given the currently enabled cargo features.
    // rustls  | native tls | result
    // -----------------------------
    // yes     | yes        | rustls
    // yes     | no         | rustls
    // no      | yes        | native_tls
    // no      | no         | no default

    use smithy_client::erase::DynConnector;

    pub fn must_have_connector() -> DynConnector {
        default_connector().expect("A connector was not available. Either set a custom connector or enable the `rustls` and `native-tls` crate features.")
    }

    #[cfg(feature = "rustls")]
    fn default_connector() -> Option<DynConnector> {
        Some(DynConnector::new(smithy_client::conns::https()))
    }

    #[cfg(all(not(feature = "rustls"), feature = "native-tls"))]
    fn default_connector() -> Option<DynConnector> {
        Some(DynConnector::new(smithy_client::conns::native_tls()))
    }

    #[cfg(not(any(feature = "rustls", feature = "native-tls")))]
    fn default_connector() -> Option<DynConnector> {
        None
    }
}
