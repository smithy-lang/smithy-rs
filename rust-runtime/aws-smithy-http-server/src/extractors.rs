use crate::model::*;
use axum::{
    async_trait,
    extract::{FromRequest, RequestParts},
};

#[async_trait]
impl<B> FromRequest<B> for RegisterServiceOperationInput
where
    B: Send,
{
    type Rejection = http::StatusCode;

    async fn from_request(_req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        Ok(RegisterServiceOperationInput(RegisterServiceInput))
    }
}

// Same thing for the other operation.

#[async_trait]
impl<B> FromRequest<B> for HealthcheckOperationInput
where
    B: Send,
{
    type Rejection = http::StatusCode;

    async fn from_request(_req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        Ok(HealthcheckOperationInput(HealthcheckInput))
    }
}
