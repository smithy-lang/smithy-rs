//! Tests for HTTP version compatibility
//!
//! These tests should pass with both HTTP 0.x (default) and HTTP 1.x (--features http-1x)

use aws_smithy_http_server::body::{boxed, collect_bytes, empty, from_bytes, to_boxed, BoxBody};
use bytes::Bytes;

#[tokio::test]
async fn test_empty_body() {
    let body = empty();
    let bytes = collect_bytes(body).await.unwrap();
    assert_eq!(bytes.len(), 0);
}

#[tokio::test]
async fn test_from_bytes() {
    let data = Bytes::from("hello world");
    let body = from_bytes(data.clone());
    let collected = collect_bytes(body).await.unwrap();
    assert_eq!(collected, data);
}

#[tokio::test]
async fn test_to_boxed_string() {
    let s = "hello world";
    let body = to_boxed(s);
    let collected = collect_bytes(body).await.unwrap();
    assert_eq!(collected, Bytes::from(s));
}

#[tokio::test]
async fn test_to_boxed_vec() {
    let vec = vec![1u8, 2, 3, 4, 5];
    let body = to_boxed(vec.clone());
    let collected = collect_bytes(body).await.unwrap();
    assert_eq!(collected.as_ref(), vec.as_slice());
}

#[tokio::test]
async fn test_to_boxed_bytes() {
    let bytes = Bytes::from("test data");
    let body = to_boxed(bytes.clone());
    let collected = collect_bytes(body).await.unwrap();
    assert_eq!(collected, bytes);
}

#[tokio::test]
async fn test_boxed_body() {
    #[cfg(not(feature = "http-1x"))]
    {
        let hyper_body = hyper_014::Body::from("test");
        let boxed: BoxBody = boxed(hyper_body);
        let collected = collect_bytes(boxed).await.unwrap();
        assert_eq!(collected, Bytes::from("test"));
    }

    #[cfg(feature = "http-1x")]
    {
        use http_body_util::Full;
        let full_body = Full::new(Bytes::from("test"));
        let boxed: BoxBody = boxed(full_body);
        let collected = collect_bytes(boxed).await.unwrap();
        assert_eq!(collected, Bytes::from("test"));
    }
}

#[test]
fn test_feature_flag_selection() {
    // This test verifies which HTTP version is selected
    #[cfg(not(feature = "http-1x"))]
    {
        println!("Running with HTTP 0.x");
        // Verify we can use HTTP 0.x types
        let _version: &str = "0.2.12";
    }

    #[cfg(feature = "http-1x")]
    {
        println!("Running with HTTP 1.x");
        // Verify we can use HTTP 1.x types
        let _version: &str = "1.x";
    }
}

/// Test that BoxBody can be used in async contexts
#[tokio::test]
async fn test_boxbody_send_sync() {
    fn assert_send<T: Send>() {}
    //fn assert_sync<T: Sync>() {}

    assert_send::<BoxBody>();
    // Note: BoxBody is explicitly UnsyncBoxBody, so it's not Sync
    // assert_sync::<BoxBody>();
}

/// Test empty body has zero length
#[tokio::test]
async fn test_empty_body_properties() {
    let body = empty();
    let bytes = collect_bytes(body).await.unwrap();

    assert_eq!(bytes.len(), 0);
    assert!(bytes.is_empty());
}

/// Test from_bytes preserves data integrity
#[tokio::test]
async fn test_from_bytes_integrity() {
    let original = Bytes::from_static(b"The quick brown fox jumps over the lazy dog");
    let body = from_bytes(original.clone());
    let collected = collect_bytes(body).await.unwrap();

    assert_eq!(collected, original);
    assert_eq!(collected.len(), original.len());
}

/// Test to_boxed with different input types
#[tokio::test]
async fn test_to_boxed_various_types() {
    // Test with &str
    let str_body = to_boxed("test");
    let str_result = collect_bytes(str_body).await.unwrap();
    assert_eq!(str_result, Bytes::from("test"));

    // Test with Vec<u8>
    let vec_body = to_boxed(vec![116, 101, 115, 116]); // "test" in bytes
    let vec_result = collect_bytes(vec_body).await.unwrap();
    assert_eq!(vec_result, Bytes::from("test"));

    // Test with Bytes
    let bytes_body = to_boxed(Bytes::from("test"));
    let bytes_result = collect_bytes(bytes_body).await.unwrap();
    assert_eq!(bytes_result, Bytes::from("test"));
}

/// Test that collect_bytes handles large bodies
#[tokio::test]
async fn test_collect_bytes_large_body() {
    let large_data = vec![42u8; 1024 * 1024]; // 1MB of data
    let body = to_boxed(large_data.clone());
    let collected = collect_bytes(body).await.unwrap();

    assert_eq!(collected.len(), 1024 * 1024);
    assert_eq!(collected.as_ref(), large_data.as_slice());
}
