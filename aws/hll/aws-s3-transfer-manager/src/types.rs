/// The target part size for an upload or download request.
#[derive(Debug, Clone)]
pub enum TargetPartSize {
    /// Automatically configure an optimal target part size based on the execution environment.
    Auto,

    /// Explicitly configured part size.
    Explicit(u64),
}

/// The concurrency settings to use for a single upload or download request.
#[derive(Debug, Clone)]
pub enum ConcurrencySetting {
    /// Automatically configure an optimal concurrency setting based on the execution environment.
    Auto,

    /// Explicitly configured concurrency setting.
    Explicit(usize),
}

/// A body size hint
#[derive(Debug, Clone, Default)]
pub(crate) struct SizeHint {
    lower: u64,
    upper: Option<u64>,
}

impl SizeHint {
    /// Set an exact size hint with upper and lower set to `size` bytes.
    pub(crate) fn exact(size: u64) -> Self {
        Self {
            lower: size,
            upper: Some(size),
        }
    }

    /// Set the lower bound on the body size
    pub(crate) fn with_lower(self, lower: u64) -> Self {
        Self { lower, ..self }
    }

    /// Set the upper bound on the body size
    pub(crate) fn with_upper(self, upper: Option<u64>) -> Self {
        Self { upper, ..self }
    }

    /// Get the lower bound of the body size
    pub(crate) fn lower(&self) -> u64 {
        self.lower
    }

    /// Get the upper bound of the body size if known.
    pub(crate) fn upper(&self) -> Option<u64> {
        self.upper
    }
}
