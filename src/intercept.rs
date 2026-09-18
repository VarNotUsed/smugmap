use libc::{c_int, c_void, mode_t, off_t, size_t, ssize_t};
use std::ffi::CStr;
use std::sync::atomic::{AtomicI32, Ordering};

static NEXT_FD: AtomicI32 = AtomicI32::new(crate::MAGIC_FD_BASE);

fn is_magic(fd: i32) -> bool {
    fd >= crate::MAGIC_FD_BASE
}

unsafe fn real_openat(dirfd: c_int, path: *const libc::c_char, flags: c_int) -> c_int {
    let sym = libc::dlsym(libc::RTLD_NEXT, c"openat".as_ptr());
    let f: unsafe extern "C" fn(c_int, *const libc::c_char, c_int) -> c_int =
        std::mem::transmute(sym);
    f(dirfd, path, flags)
}

unsafe fn real_open(path: *const libc::c_char, flags: c_int, mode: mode_t) -> c_int {
    let sym = libc::dlsym(libc::RTLD_NEXT, c"open64".as_ptr());
    let f: unsafe extern "C" fn(*const libc::c_char, c_int, mode_t) -> c_int =
        std::mem::transmute(sym);
    f(path, flags, mode)
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
    // Only intercept absolute paths opened relative to CWD; pass through fd-relative opens
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
            (*stat).st_blksize = 4096;
            (*stat).st_blocks = (state.size / 512 + 1) as libc::blkcnt_t;
            return 0;
        }
    }
    let sym = libc::dlsym(libc::RTLD_NEXT, c"fstat".as_ptr());
    let f: unsafe extern "C" fn(c_int, *mut libc::stat) -> c_int = std::mem::transmute(sym);
    f(fd, stat)
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
        let sym = libc::dlsym(libc::RTLD_NEXT, c"pread".as_ptr());
        let f: unsafe extern "C" fn(c_int, *mut c_void, size_t, off_t) -> ssize_t =
            std::mem::transmute(sym);
        return f(fd, buf, count, offset);
    }

    let url = match crate::files().lock().unwrap().get(&fd) {
        Some(s) => s.url.clone(),
        None => {
            let sym = libc::dlsym(libc::RTLD_NEXT, c"pread".as_ptr());
            let f: unsafe extern "C" fn(c_int, *mut c_void, size_t, off_t) -> ssize_t =
                std::mem::transmute(sym);
            return f(fd, buf, count, offset);
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
pub unsafe extern "C" fn close(fd: c_int) -> c_int {
    crate::files().lock().unwrap().remove(&fd);
    let sym = libc::dlsym(libc::RTLD_NEXT, c"close".as_ptr());
    let f: unsafe extern "C" fn(c_int) -> c_int = std::mem::transmute(sym);
    f(fd)
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
    ) -> *mut c_void = {
        let sym = libc::dlsym(libc::RTLD_NEXT, c"mmap".as_ptr());
        std::mem::transmute(sym)
    };

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
        // pread() fallback still works for non-mmap access
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
    let sym = libc::dlsym(libc::RTLD_NEXT, c"munmap".as_ptr());
    let f: unsafe extern "C" fn(*mut c_void, size_t) -> c_int = std::mem::transmute(sym);
    f(addr, length)
}
