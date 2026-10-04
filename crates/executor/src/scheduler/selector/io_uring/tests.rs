// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::*;

#[test]
fn queued_request_returns_its_buffer_if_the_selector_exits() {
    let (reply, mut result) = oneshot::channel();
    let buffer = vec![7; 32];
    let allocation = buffer.as_ptr();
    let request = Request {
        identifier: 0,
        resource: Resource::File(
            Arc::new(File::open(std::env::current_exe().unwrap()).unwrap()),
            0,
        ),
        buffer,
        write: false,
        reply: Some(reply),
        cancelling: false,
    };
    drop(request);
    let (result, buffer) = result.try_recv().unwrap().unwrap();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(buffer, vec![7; 32]);
    assert_eq!(buffer.as_ptr(), allocation);
}
