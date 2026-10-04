// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use std::process::Command;

#[test]
fn executable_lists_discovered_tasks() {
    let output = Command::new(env!("CARGO_BIN_EXE_socketry-rust-tasks"))
        .arg("--list")
        .output()
        .expect("run Bake task executable");

    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).expect("task listing is UTF-8");
    assert!(output.contains("cargo:release"));
    assert!(output.contains("agent:context:install"));
    assert!(output.contains("test:coverage"));
}
