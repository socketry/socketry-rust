// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use super::{wrap_accepted, wrap_listener, wrap_socket};
use std::io;

fn injected_error<T>(result: io::Result<T>) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("injected error was not preserved"),
    };

    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "injected selector failure");
}

fn selector_error<T>() -> io::Result<T> {
    Err(io::Error::other("injected selector failure"))
}

#[test]
fn registration_and_network_errors_are_preserved() {
    injected_error(wrap_socket(selector_error()));
    injected_error(wrap_listener(selector_error()));
    injected_error(wrap_accepted(selector_error()));
}
