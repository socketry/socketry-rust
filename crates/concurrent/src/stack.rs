use std::io;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;

const MIN_STACK_SIZE: usize = 16 * 1024;

/// A guarded stack allocation, kept on its owning OS thread.
pub struct Stack {
    mapping: NonNull<u8>,
    mapping_size: usize,
    base: NonNull<u8>,
    size: usize,
    _thread_affine: PhantomData<Rc<()>>,
}

impl Stack {
    /// Allocate a stack with at least the requested number of usable bytes.
    pub fn new(size: usize) -> io::Result<Self> {
        if size < MIN_STACK_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("stack size must be at least {MIN_STACK_SIZE} bytes"),
            ));
        }

        // SAFETY: sysconf has no pointer arguments and returns the OS page size.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size <= 0 {
            return Err(io::Error::last_os_error());
        }
        let page_size = page_size as usize;
        let usable_size = size
            .checked_add(page_size - 1)
            .map(|n| n / page_size * page_size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stack size overflow"))?;
        let guards_size = page_size
            .checked_mul(2)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stack size overflow"))?;
        let mapping_size = usable_size
            .checked_add(guards_size)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stack size overflow"))?;

        // SAFETY: mmap reserves a private anonymous region; it is released in Drop.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                mapping_size,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: mmap returned a valid mapping of mapping_size bytes.
        let mapping = unsafe { NonNull::new_unchecked(raw.cast::<u8>()) };
        // SAFETY: the first guard page precedes this usable range.
        let base = unsafe { NonNull::new_unchecked(mapping.as_ptr().add(page_size)) };
        // SAFETY: this range is wholly within the mapping and excludes both guards.
        if unsafe {
            libc::mprotect(
                base.as_ptr().cast(),
                usable_size,
                libc::PROT_READ | libc::PROT_WRITE,
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            // SAFETY: this releases the complete mapping returned by mmap above.
            unsafe { libc::munmap(mapping.as_ptr().cast(), mapping_size) };
            return Err(error);
        }

        Ok(Self {
            mapping,
            mapping_size,
            base,
            size: usable_size,
            _thread_affine: PhantomData,
        })
    }

    /// Usable stack size, rounded up to a whole number of pages.
    pub fn size(&self) -> usize {
        self.size
    }

    pub(crate) fn base(&self) -> *mut u8 {
        self.base.as_ptr()
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        // SAFETY: this mapping is owned by this Stack and has not been released.
        unsafe { libc::munmap(self.mapping.as_ptr().cast(), self.mapping_size) };
    }
}

impl std::fmt::Debug for Stack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Stack")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}
