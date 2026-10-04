// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::*;
use std::future::ready;
use std::task::{Context, Poll, Waker};

fn runtime() -> ::tokio::runtime::Runtime {
    ::tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn socket_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}

fn expect_error<ResultType>(result: io::Result<ResultType>) -> io::Error {
    match result {
        Ok(_) => panic!("operation unexpectedly succeeded"),
        Err(error) => error,
    }
}

fn shared(runtime: &::tokio::runtime::Runtime) -> Arc<Shared> {
    Arc::new(Shared {
        runtime: runtime.handle().clone(),
        closed: AtomicBool::new(false),
        registry: Mutex::new(Registry {
            closed: false,
            next_identifier: 0,
            tasks: HashMap::new(),
        }),
        root: Arc::new(Owner::new()),
    })
}

fn task_state(owner: Arc<Owner>, cancelled: bool) -> Arc<TaskState> {
    Arc::new(TaskState {
        owner,
        cancelled: AtomicBool::new(cancelled),
    })
}

#[test]
fn completion_removes_registration_and_notifies_owner() {
    let runtime = runtime();
    let shared = shared(&runtime);
    let owner = Arc::new(Owner::new());
    owner.remaining.store(1, Ordering::Release);
    let state = task_state(Arc::clone(&owner), false);
    shared.registry.lock().unwrap().tasks.insert(
        7,
        Registration {
            state: Arc::clone(&state),
            abort: None,
        },
    );

    drop(Completion {
        scheduler: Arc::downgrade(&shared),
        identifier: 7,
        state,
    });

    assert!(shared.registry.lock().unwrap().tasks.is_empty());
    assert!(owner.is_empty());
}

#[test]
fn completion_notifies_owner_after_scheduler_is_gone() {
    let runtime = runtime();
    let shared = shared(&runtime);
    let owner = Arc::new(Owner::new());
    owner.remaining.store(1, Ordering::Release);
    let completion = Completion {
        scheduler: Arc::downgrade(&shared),
        identifier: 7,
        state: task_state(Arc::clone(&owner), false),
    };
    drop(shared);

    drop(completion);

    assert!(owner.is_empty());
}

#[test]
fn tracked_future_does_not_poll_after_cancellation() {
    let owner = Arc::new(Owner::new());
    owner.remaining.store(1, Ordering::Release);
    let state = task_state(Arc::clone(&owner), true);
    let mut context = Context::from_waker(Waker::noop());

    {
        let mut future = Box::pin(TrackedFuture {
            future: ready(42),
            completion: Completion {
                scheduler: Weak::new(),
                identifier: 0,
                state,
            },
        });
        assert!(matches!(
            future.as_mut().poll(&mut context),
            Poll::Ready(Err(TaskError::Cancelled))
        ));
    }
    assert!(owner.is_empty());
}

#[test]
fn delayed_abort_respects_an_earlier_cancellation_request() {
    let runtime = runtime();
    let task = runtime.spawn(std::future::pending::<()>());
    abort_if_cancelled(false, || panic!("an active task should not be aborted"));
    abort_if_cancelled(true, || task.abort());

    assert!(runtime.block_on(task).is_err());
}

#[test]
fn close_without_cancellation_only_closes_admission() {
    let runtime = runtime();
    let shared = shared(&runtime);
    let owner = Arc::new(Owner::new());

    shared.close(Some(&owner), false);

    assert!(owner.closed.load(Ordering::Acquire));
    assert!(!shared.closed.load(Ordering::Acquire));
}

#[test]
fn poisoned_registry_locks_do_not_break_spawn_or_shutdown() {
    let runtime = runtime();
    let scheduler = Scheduler::new(runtime.handle().clone());
    let shared = Arc::clone(&scheduler.handle.shared);
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _registry = shared.registry.lock().unwrap();
        panic!("poison registry mutex");
    }));
    assert!(poisoned.is_err());

    let task = scheduler.spawn(async { 42 }).unwrap();
    assert_eq!(runtime.block_on(task).unwrap(), 42);
    runtime.block_on(scheduler.shutdown());
    assert!(shared.closed.load(Ordering::Acquire));
}

#[test]
fn poisoned_registry_is_recovered_when_a_task_attempts_self_shutdown() {
    let runtime = runtime();
    let scheduler = Scheduler::new(runtime.handle().clone());
    let shared = Arc::clone(&scheduler.handle.shared);
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _registry = shared.registry.lock().unwrap();
        panic!("poison registry mutex");
    }));
    assert!(poisoned.is_err());

    let task = scheduler
        .handle()
        .spawn(async move { scheduler.shutdown().await })
        .unwrap();
    assert!(matches!(
        runtime.block_on(task),
        Err(TaskError::Panicked(_))
    ));
}

#[test]
fn task_identifier_exhaustion_is_reported() {
    let runtime = runtime();
    let scheduler = Scheduler::new(runtime.handle().clone());
    scheduler
        .handle
        .shared
        .registry
        .lock()
        .unwrap()
        .next_identifier = u64::MAX;

    assert!(matches!(
        scheduler.spawn(async {}),
        Err(SpawnError::IdentifiersExhausted)
    ));
}

#[test]
fn socket_registration_preserves_configuration_and_conversion_errors() {
    let runtime = runtime();
    let (socket, _peer) = socket_pair();
    let error = expect_error(register_socket_with(
        runtime.handle(),
        socket,
        |_| Err(io::Error::other("socket mode failed")),
        |_| panic!("conversion should not run after configuration fails"),
    ));
    assert_eq!(error.to_string(), "socket mode failed");

    let (socket, _peer) = socket_pair();
    let error = expect_error(register_socket_with(
        runtime.handle(),
        socket,
        |_| Ok(()),
        |_| Err(io::Error::other("socket conversion failed")),
    ));
    assert_eq!(error.to_string(), "socket conversion failed");
}

#[test]
fn listener_registration_preserves_configuration_and_conversion_errors() {
    let runtime = runtime();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let error = expect_error(register_listener_with(
        runtime.handle(),
        listener,
        |_| Err(io::Error::other("listener mode failed")),
        |_| panic!("conversion should not run after configuration fails"),
    ));
    assert_eq!(error.to_string(), "listener mode failed");

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let error = expect_error(register_listener_with(
        runtime.handle(),
        listener,
        |_| Ok(()),
        |_| Err(io::Error::other("listener conversion failed")),
    ));
    assert_eq!(error.to_string(), "listener conversion failed");
}

#[test]
fn connection_and_accept_errors_are_propagated() {
    let runtime = runtime();
    let error = runtime.block_on(connect_with(
        runtime.handle().clone(),
        runtime.handle().id(),
        ready(Err(io::Error::other("connect failed"))),
    ));
    assert_eq!(expect_error(error).to_string(), "connect failed");

    let error = runtime.block_on(accept_with(
        runtime.handle().clone(),
        runtime.handle().id(),
        ready(Err(io::Error::other("accept failed"))),
    ));
    assert_eq!(expect_error(error).to_string(), "accept failed");
}

#[test]
fn poisoned_file_buffers_are_recovered_for_operations_and_return() {
    let buffer = Mutex::new(vec![1]);
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _buffer = buffer.lock().unwrap();
        panic!("poison file buffer");
    }));
    assert!(poisoned.is_err());

    assert_eq!(
        with_file_buffer(&buffer, |buffer| {
            buffer.push(2);
            Ok(buffer.len())
        })
        .unwrap(),
        2
    );
    assert_eq!(take_file_buffer(&buffer), vec![1, 2]);
}

#[test]
fn read_retries_interrupted_and_would_block_operations() {
    let runtime = runtime();
    runtime.block_on(async {
        let mut attempts = 0;
        let mut readiness_waits = 0;
        let mut buffer = [0; 1];
        let result = read_with(
            &mut buffer,
            |buffer| {
                attempts += 1;
                match attempts {
                    1 => Err(io::ErrorKind::WouldBlock.into()),
                    2 => Err(io::ErrorKind::Interrupted.into()),
                    _ => {
                        buffer[0] = 42;
                        Ok(1)
                    }
                }
            },
            || {
                readiness_waits += 1;
                ready(Ok(()))
            },
        )
        .await;

        assert_eq!(result.unwrap(), 1);
        assert_eq!(buffer, [42]);
        assert_eq!(attempts, 3);
        assert_eq!(readiness_waits, 1);
    });
}

#[test]
fn read_returns_readiness_errors_and_non_retryable_errors() {
    let runtime = runtime();
    runtime.block_on(async {
        let mut buffer = [0; 1];
        let result = read_with(
            &mut buffer,
            |_| Err(io::ErrorKind::WouldBlock.into()),
            || ready(Err(io::Error::other("readiness failed"))),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "readiness failed");

        let result = read_with(
            &mut buffer,
            |_| Err(io::Error::other("read failed")),
            || ready(Ok(())),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "read failed");
    });
}

#[test]
fn write_retries_interrupted_and_would_block_operations() {
    let runtime = runtime();
    runtime.block_on(async {
        let mut attempts = 0;
        let mut readiness_waits = 0;
        let result = write_with(
            &[42],
            |_| {
                attempts += 1;
                match attempts {
                    1 => Err(io::ErrorKind::WouldBlock.into()),
                    2 => Err(io::ErrorKind::Interrupted.into()),
                    _ => Ok(1),
                }
            },
            || {
                readiness_waits += 1;
                ready(Ok(()))
            },
        )
        .await;

        assert_eq!(result.unwrap(), 1);
        assert_eq!(attempts, 3);
        assert_eq!(readiness_waits, 1);
    });
}

#[test]
fn write_returns_readiness_errors_and_non_retryable_errors() {
    let runtime = runtime();
    runtime.block_on(async {
        let result = write_with(
            &[42],
            |_| Err(io::ErrorKind::WouldBlock.into()),
            || ready(Err(io::Error::other("readiness failed"))),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "readiness failed");

        let result = write_with(
            &[42],
            |_| Err(io::Error::other("write failed")),
            || ready(Ok(())),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "write failed");
    });
}

#[test]
fn file_operation_results_preserve_errors_and_resume_panics() {
    let runtime = runtime();
    assert_eq!(file_operation_result(Ok(Ok(42))).unwrap(), 42);
    assert_eq!(
        file_operation_result::<usize>(Ok(Err(io::Error::other("file failed"))))
            .unwrap_err()
            .to_string(),
        "file failed"
    );

    let cancelled = runtime.spawn(std::future::pending::<()>());
    cancelled.abort();
    let cancelled = runtime.block_on(cancelled).unwrap_err();
    assert_eq!(
        file_operation_result::<usize>(Err(cancelled))
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );

    let panicked = runtime.spawn(async { panic!("blocking task panicked") });
    let panicked = runtime.block_on(panicked).unwrap_err();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            file_operation_result::<usize>(Err(panicked))
        }))
        .is_err()
    );
}
