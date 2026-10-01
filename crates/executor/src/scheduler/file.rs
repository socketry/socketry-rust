// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

//! Blocking positioned file operations shared by selector adapters.
use std::fs::File;
use std::io;

pub(crate) fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    if offset > i64::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file offset exceeds i64::MAX",
        ));
    }
    loop {
        #[cfg(unix)]
        let result = {
            use std::os::unix::fs::FileExt;
            file.read_at(buffer, offset)
        };
        #[cfg(windows)]
        let result = {
            use std::os::windows::fs::FileExt;
            file.seek_read(buffer, offset)
        };
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

pub(crate) fn write_at(file: &File, buffer: &[u8], offset: u64) -> io::Result<usize> {
    if offset > i64::MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file offset exceeds i64::MAX",
        ));
    }
    loop {
        #[cfg(unix)]
        let result = {
            use std::os::unix::fs::FileExt;
            file.write_at(buffer, offset)
        };
        #[cfg(windows)]
        let result = {
            use std::os::windows::fs::FileExt;
            file.seek_write(buffer, offset)
        };
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}
