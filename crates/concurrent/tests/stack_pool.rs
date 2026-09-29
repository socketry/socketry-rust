use socketry_concurrent::{Pool, Stack};

const STACK_SIZE: usize = 64 * 1024;

#[test]
fn stack_provides_at_least_the_requested_capacity() {
    let stack = Stack::new(STACK_SIZE + 1).expect("stack should be allocated");

    assert!(stack.size() >= STACK_SIZE + 1);
}

#[test]
fn undersized_stacks_are_rejected() {
    let error = Stack::new(1).expect_err("stack smaller than the minimum should be rejected");

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn pool_reuses_and_limits_cached_stacks() {
    let mut pool = Pool::with_cache_limit(STACK_SIZE, 1);
    let first_stack = pool.acquire().expect("stack should be allocated");

    pool.recycle(first_stack);
    assert_eq!(pool.cached(), 1);

    let reused_stack = pool.acquire().expect("cached stack should be reused");
    assert_eq!(pool.cached(), 0);

    pool.recycle(reused_stack);
    pool.recycle(Stack::new(STACK_SIZE).expect("second stack should be allocated"));
    assert_eq!(pool.cached(), 1);
}
