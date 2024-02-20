use aws_smithy_async::rt::sleep::{AsyncSleep, Sleep};

/// A struct implementing the AsyncSleep trait that can be used in
/// Wasm environments
#[derive(Debug, Clone)]
pub struct WasmSleep;
impl AsyncSleep for WasmSleep {
    fn sleep(&self, duration: std::time::Duration) -> Sleep {
        Sleep::new(Box::pin(async move {
            tokio::time::sleep(duration).await;
        }))
    }
}
