// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

//! Native I/O selectors. Platform modules select the OS mechanism at compile
//! time; Tokio supplies a separate runtime adapter in `scheduler::tokio`.
#[cfg(feature = "native")]
pub mod readiness;

#[cfg(all(feature = "native", any(target_os = "linux", target_os = "android")))]
pub mod epoll;

#[cfg(all(
    feature = "native",
    any(
        target_vendor = "apple",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly"
    )
))]
pub mod kqueue;

#[cfg(all(feature = "native", windows))]
pub mod iocp;

#[cfg(all(feature = "io-uring", target_os = "linux"))]
pub mod io_uring;

#[cfg(all(feature = "io-uring", target_os = "linux"))]
pub use io_uring::Selector as DefaultSelector;

#[cfg(all(
    feature = "native",
    not(all(feature = "io-uring", target_os = "linux"))
))]
pub use readiness::Selector as DefaultSelector;
