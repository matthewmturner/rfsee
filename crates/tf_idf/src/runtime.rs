use std::{num::NonZeroUsize, thread::available_parallelism};

use crate::threadpool::ThreadPool;

/// Holds the resources shared by operations that run in parallel, such as loading RFCs.
///
/// The runtime owns a single thread pool that is created once and reused across operations, so
/// the level of parallelism is decided in one place by whoever constructs the runtime.
pub struct Runtime {
    parallelism: NonZeroUsize,
    pool: ThreadPool,
}

impl Runtime {
    /// Fallback used when the platform cannot report its available parallelism.
    const FALLBACK_PARALLELISM: usize = 1;

    /// Create a runtime with a thread pool of the given size.
    pub fn new(parallelism: NonZeroUsize) -> Self {
        Self {
            parallelism,
            pool: ThreadPool::new(parallelism.get()),
        }
    }

    /// The parallelism reported by the platform, i.e. `std::thread::available_parallelism`,
    /// falling back to a single thread when it cannot be determined.
    pub fn available_parallelism() -> NonZeroUsize {
        available_parallelism().unwrap_or_else(|_| {
            NonZeroUsize::new(Self::FALLBACK_PARALLELISM).expect("fallback is non-zero")
        })
    }

    /// Number of worker threads in the runtime's pool.
    pub fn parallelism(&self) -> NonZeroUsize {
        self.parallelism
    }

    pub(crate) fn pool(&self) -> &ThreadPool {
        &self.pool
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(Self::available_parallelism())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::Runtime;

    #[test]
    fn default_runtime_uses_available_parallelism() {
        let runtime = Runtime::default();
        let expected =
            std::thread::available_parallelism().unwrap_or_else(|_| NonZeroUsize::new(1).unwrap());
        assert_eq!(runtime.parallelism(), expected);
    }

    #[test]
    fn runtime_uses_requested_parallelism() {
        let runtime = Runtime::new(NonZeroUsize::new(3).unwrap());
        assert_eq!(runtime.parallelism().get(), 3);
    }
}
