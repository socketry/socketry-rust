use crate::stack::Stack;
use std::io;

/// A cache of guarded stacks for scheduler tasks.
pub struct Pool {
    stack_size: usize,
    max_cached: usize,
    free: Vec<Stack>,
}

impl Pool {
    /// Create an empty pool. Stacks are allocated on demand.
    pub fn new(stack_size: usize) -> Self {
        Self::with_cache_limit(stack_size, usize::MAX)
    }

    /// Create a pool that retains at most max_cached unused stacks.
    pub fn with_cache_limit(stack_size: usize, max_cached: usize) -> Self {
        Self {
            stack_size,
            max_cached,
            free: Vec::new(),
        }
    }

    /// Take a reusable stack or allocate one.
    pub fn acquire(&mut self) -> io::Result<Stack> {
        match self.free.pop() {
            Some(stack) => Ok(stack),
            None => Stack::new(self.stack_size),
        }
    }

    /// Return an unused stack to the pool.
    pub fn recycle(&mut self, stack: Stack) {
        if self.free.len() < self.max_cached {
            self.free.push(stack);
        }
    }

    /// Number of stacks currently cached for reuse.
    pub fn cached(&self) -> usize {
        self.free.len()
    }

    /// Usable size configured for newly allocated stacks.
    pub fn stack_size(&self) -> usize {
        self.stack_size
    }
}
