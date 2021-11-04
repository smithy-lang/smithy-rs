use axum::response::IntoResponse;
use axum::BoxError;
use axum::{
    body::{box_body, BoxBody},
    Error,
};
use bytes::Bytes;
use http_body::Full;
use std::convert::Infallible;

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "Extensions taken by other extractor"]
    /// Rejection used if the request extension has been taken by another
    /// extractor.
    pub struct ExtensionsAlreadyExtracted;
}

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "Headers taken by other extractor"]
    /// Rejection used if the headers has been taken by another extractor.
    pub struct HeadersAlreadyExtracted;
}

define_rejection! {
    #[status = BAD_REQUEST]
    #[body = "Failed to parse the request body as JSON"]
    /// Rejection type for [`Json`](super::Json).
    pub struct InvalidJsonBody(Error);
}

define_rejection! {
    #[status = BAD_REQUEST]
    #[body = "Expected request with `Content-Type: application/json`"]
    /// Rejection type for [`Json`](super::Json) used if the `Content-Type`
    /// header is missing.
    pub struct MissingJsonContentType;
}

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "Missing request extension"]
    /// Rejection type for [`Extension`](super::Extension) if an expected
    /// request extension was not found.
    pub struct MissingExtension(Error);
}

define_rejection! {
    #[status = BAD_REQUEST]
    #[body = "Failed to buffer the request body"]
    /// Rejection type for extractors that buffer the request body. Used if the
    /// request body cannot be buffered due to an error.
    pub struct FailedToBufferBody(Error);
}

define_rejection! {
    #[status = BAD_REQUEST]
    #[body = "Request body didn't contain valid UTF-8"]
    /// Rejection type used when buffering the request into a [`String`] if the
    /// body doesn't contain valid UTF-8.
    pub struct InvalidUtf8(Error);
}

define_rejection! {
    #[status = PAYLOAD_TOO_LARGE]
    #[body = "Request payload is too large"]
    /// Rejection type for [`ContentLengthLimit`](super::ContentLengthLimit) if
    /// the request body is too large.
    pub struct PayloadTooLarge;
}

define_rejection! {
    #[status = LENGTH_REQUIRED]
    #[body = "Content length header is required"]
    /// Rejection type for [`ContentLengthLimit`](super::ContentLengthLimit) if
    /// the request is missing the `Content-Length` header or it is invalid.
    pub struct LengthRequired;
}

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "No url params found for matched route. This is a bug in axum. Please open an issue"]
    /// Rejection type used if you try and extract the URL params more than once.
    pub struct MissingRouteParams;
}

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "Cannot have two request body extractors for a single handler"]
    /// Rejection type used if you try and extract the request body more than
    /// once.
    pub struct BodyAlreadyExtracted;
}

define_rejection! {
    #[status = BAD_REQUEST]
    #[body = "Form requests must have `Content-Type: x-www-form-urlencoded`"]
    /// Rejection type used if you try and extract the request more than once.
    pub struct InvalidFormContentType;
}

define_rejection! {
    #[status = INTERNAL_SERVER_ERROR]
    #[body = "No matched path found"]
    /// Rejection if no matched path could be found.
    ///
    /// See [`MatchedPath`](super::MatchedPath) for more details.
    pub struct MatchedPathMissing;
}

composite_rejection! {
    pub enum RestJson1Error {
        BodyAlreadyExtracted,
        FailedToBufferBody,
        HeadersAlreadyExtracted,
        InvalidJsonBody,
        InvalidUtf8,
        LengthRequired,
        MissingJsonContentType,
        MissingRouteParams,
        PayloadTooLarge,
    }
}
