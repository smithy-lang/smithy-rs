/// A boxed [std::error::Error] trait object that's [Send] and [Sync]
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
