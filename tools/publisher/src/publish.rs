use crates_io_api::{AsyncClient, Error};
use lazy_static::lazy_static;
use std::time::Duration;

lazy_static! {
    pub static ref CRATES_IO_CLIENT: AsyncClient = AsyncClient::new(
        "AWS_RUST_SDK_PUBLISHER (aws-sdk-rust@amazon.com)",
        Duration::from_secs(1)
    )
    .expect("valid client");
}

/// Return `true` if there is at least one version published on crates.io associated with
/// the specified crate name.
#[tracing::instrument]
pub async fn has_been_published_on_crates_io(crate_name: &str) -> anyhow::Result<bool> {
    match CRATES_IO_CLIENT.get_crate(crate_name).await {
        Ok(_) => Ok(true),
        Err(Error::NotFound(_)) => Ok(false),
        Err(e) => Err(e.into()),
    }
}
