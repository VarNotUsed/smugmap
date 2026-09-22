use std::ffi::CStr;
use std::sync::atomic::{AtomicI32, Ordering};

use libc::{c_int, c_void, mode_t, off_t, size_t, ssize_t};

// POSIX stat(2) counts blocks in 512-byte units regardless of filesystem block size.
const STAT_BLOCK_SIZE: u64 = 512;
// Hint to callers for efficient sequential I/O; matches typical page/cluster size.
const PREFERRED_IO_BLOCK: i32 = 4096;

macro_rules! call_real {
    ($sym:literal, $ty:ty, $($arg:expr),*) => {{
        let f: $ty = std::mem::transmute(
            libc::dlsym(libc::RTLD_NEXT, concat!($sym, "\0").as_ptr() as *const libc::c_char)
        );
        f($($arg),*)
    }};
}

#[cfg(target_os = "linux")]
fn set_errno(e: c_int) {
    unsafe { *libc::__errno_location() = e };
}
#[cfg(not(target_os = "linux"))]
fn set_errno(_: c_int) {}

static NEXT_FD: AtomicI32 = AtomicI32::new(crate::MAGIC_FD_BASE);

fn is_magic(fd: i32) -> bool {
    fd >= crate::MAGIC_FD_BASE
}

unsafe fn real_openat(dirfd: c_int, path: *const libc::c_char, flags: c_int) -> c_int {
    call_real!(
        "openat",
        unsafe extern "C" fn(c_int, *const libc::c_char, c_int) -> c_int,
        dirfd,
        path,
        flags
    )
}

unsafe fn real_open(path: *const libc::c_char, flags: c_int, mode: mode_t) -> c_int {
    call_real!(
        "open64",
        unsafe extern "C" fn(*const libc::c_char, c_int, mode_t) -> c_int,
        path,
        flags,
        mode
    )
}

unsafe fn intercept_open(path: *const libc::c_char, flags: c_int) -> c_int {
    let path_str = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return real_open(path, flags, 0o666),
    };

    let cfg = match crate::config::global() {
        Some(c) => c,
        None => return real_open(path, flags, 0o666),
    };

    let entry = match cfg.find(path_str) {
        Some(e) => e,
        None => return real_open(path, flags, 0o666),
    };

    let size = match crate::http::head_size(&entry.url) {
        Ok(s) => s,
        Err(e) => {
            if !crate::quiet() {
                eprintln!("[smugmap] HEAD failed for {path_str}: {e}");
            }
            return -libc::EIO;
        }
    };

    let fd = NEXT_FD.fetch_add(1, Ordering::Relaxed);
    crate::files().lock().unwrap().insert(
        fd,
        crate::FileState {
            url: entry.url.clone(),
            size,
            readahead: entry.readahead,
            offset: 0,
            mmap_ptr: 0,
            mmap_len: 0,
        },
    );
    fd
}

// Intercept all open variants — Rust stdlib on Linux calls openat(AT_FDCWD,...)
// open/open64 for legacy callers, openat/openat64 for modern glibc/Rust stdlib
#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn open(path: *const libc::c_char, flags: c_int) -> c_int {
    intercept_open(path, flags)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn open64(path: *const libc::c_char, flags: c_int) -> c_int {
    intercept_open(path, flags)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat(dirfd: c_int, path: *const libc::c_char, flags: c_int) -> c_int {
    // Only intercept CWD-relative opens; pass through fd-relative opens
    if dirfd != libc::AT_FDCWD {
        return real_openat(dirfd, path, flags);
    }
    intercept_open(path, flags)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn openat64(dirfd: c_int, path: *const libc::c_char, flags: c_int) -> c_int {
    if dirfd != libc::AT_FDCWD {
        return real_openat(dirfd, path, flags);
    }
    intercept_open(path, flags)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fstat(fd: c_int, stat: *mut libc::stat) -> c_int {
    if is_magic(fd) {
        let files = crate::files().lock().unwrap();
        if let Some(state) = files.get(&fd) {
            *stat = std::mem::zeroed();
            (*stat).st_size = state.size as libc::off_t;
            (*stat).st_mode = libc::S_IFREG | 0o444;
            (*stat).st_blksize = PREFERRED_IO_BLOCK as libc::blksize_t;
            (*stat).st_blocks = (state.size / STAT_BLOCK_SIZE + 1) as libc::blkcnt_t;
            return 0;
        }
    }
    call_real!(
        "fstat",
        unsafe extern "C" fn(c_int, *mut libc::stat) -> c_int,
        fd,
        stat
    )
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pread(
    fd: c_int,
    buf: *mut c_void,
    count: size_t,
    offset: off_t,
) -> ssize_t {
    if !is_magic(fd) {
        return call_real!(
            "pread",
            unsafe extern "C" fn(c_int, *mut c_void, size_t, off_t) -> ssize_t,
            fd,
            buf,
            count,
            offset
        );
    }

    if count == 0 {
        return 0;
    }

    let url = match crate::files().lock().unwrap().get(&fd) {
        Some(s) => s.url.clone(),
        None => {
            return call_real!(
                "pread",
                unsafe extern "C" fn(c_int, *mut c_void, size_t, off_t) -> ssize_t,
                fd,
                buf,
                count,
                offset
            );
        }
    };

    let end = offset as u64 + count as u64 - 1;
    match crate::http::fetch_range(&url, offset as u64, end) {
        Ok(data) => {
            let n = data.len().min(count);
            std::ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, n);
            n as ssize_t
        }
        Err(e) => {
            if !crate::quiet() {
                eprintln!("[smugmap] pread fetch failed: {e}");
            }
            -1
        }
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn read(fd: c_int, buf: *mut c_void, count: size_t) -> ssize_t {
    if !is_magic(fd) {
        return call_real!(
            "read",
            unsafe extern "C" fn(c_int, *mut c_void, size_t) -> ssize_t,
            fd,
            buf,
            count
        );
    }

    if count == 0 {
        return 0;
    }

    let (url, offset, size) = {
        let files = crate::files().lock().unwrap();
        match files.get(&fd) {
            Some(s) => (s.url.clone(), s.offset, s.size),
            None => return 0,
        }
    };

    if offset >= size {
        return 0; // EOF
    }
    let count = count.min((size - offset) as usize);
    let end = offset + count as u64 - 1;

    match crate::http::fetch_range(&url, offset, end) {
        Ok(data) => {
            let n = data.len().min(count);
            std::ptr::copy_nonoverlapping(data.as_ptr(), buf as *mut u8, n);
            if let Some(s) = crate::files().lock().unwrap().get_mut(&fd) {
                s.offset += n as u64;
            }
            n as ssize_t
        }
        Err(e) => {
            if !crate::quiet() {
                eprintln!("[smugmap] read fetch failed: {e}");
            }
            -1
        }
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn close(fd: c_int) -> c_int {
    if crate::files().lock().unwrap().remove(&fd).is_some() {
        return 0;
    }
    call_real!("close", unsafe extern "C" fn(c_int) -> c_int, fd)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mmap(
    addr: *mut c_void,
    length: size_t,
    prot: c_int,
    flags: c_int,
    fd: c_int,
    offset: off_t,
) -> *mut c_void {
    let real_mmap: unsafe extern "C" fn(
        *mut c_void,
        size_t,
        c_int,
        c_int,
        c_int,
        off_t,
    ) -> *mut c_void = std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, c"mmap".as_ptr()));

    if !is_magic(fd) {
        return real_mmap(addr, length, prot, flags, fd, offset);
    }

    let (url, readahead) = match crate::files().lock().unwrap().get(&fd) {
        Some(s) => (s.url.clone(), s.readahead),
        None => return real_mmap(addr, length, prot, flags, fd, offset),
    };

    let ptr = real_mmap(
        addr,
        length,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
        -1,
        0,
    );

    if ptr == libc::MAP_FAILED {
        return libc::MAP_FAILED;
    }

    if let Err(e) = crate::uffd::register(ptr, length, url, readahead) {
        if !crate::quiet() {
            eprintln!("[smugmap] uffd register failed: {e}");
        }
        let munmap_fn: unsafe extern "C" fn(*mut c_void, size_t) -> c_int =
            std::mem::transmute(libc::dlsym(libc::RTLD_NEXT, c"munmap".as_ptr()));
        munmap_fn(ptr, length);
        set_errno(libc::EIO);
        return libc::MAP_FAILED;
    }

    if let Some(state) = crate::files().lock().unwrap().get_mut(&fd) {
        state.mmap_ptr = ptr as usize;
        state.mmap_len = length;
    }

    ptr
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn munmap(addr: *mut c_void, length: size_t) -> c_int {
    crate::uffd::unregister(addr, length);
    call_real!(
        "munmap",
        unsafe extern "C" fn(*mut c_void, size_t) -> c_int,
        addr,
        length
    )
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lseek(fd: c_int, offset: off_t, whence: c_int) -> off_t {
    if !is_magic(fd) {
        return call_real!(
            "lseek",
            unsafe extern "C" fn(c_int, off_t, c_int) -> off_t,
            fd,
            offset,
            whence
        );
    }

    let mut files = crate::files().lock().unwrap();
    let Some(state) = files.get_mut(&fd) else {
        return call_real!(
            "lseek",
            unsafe extern "C" fn(c_int, off_t, c_int) -> off_t,
            fd,
            offset,
            whence
        );
    };

    let new_offset = match whence {
        libc::SEEK_SET => offset,
        libc::SEEK_CUR => state.offset as i64 + offset,
        libc::SEEK_END => state.size as i64 + offset,
        _ => {
            set_errno(libc::EINVAL);
            return -1;
        }
    };

    if new_offset < 0 {
        set_errno(libc::EINVAL);
        return -1;
    }

    state.offset = new_offset as u64;
    new_offset as off_t
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lseek64(fd: c_int, offset: off_t, whence: c_int) -> off_t {
    lseek(fd, offset, whence)
}
