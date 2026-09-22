use std::alloc::System;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[global_allocator]
static A: System = System;

pub(crate) mod config;
pub(crate) mod http;
pub(crate) mod intercept;
pub(crate) mod sigv4;
pub(crate) mod uffd;

pub(crate) struct FileState {
    pub url: String,
    pub size: u64,
    pub readahead: usize,
    pub mmap_ptr: usize,
    pub mmap_len: usize,
}

static FILES_INNER: OnceLock<Mutex<HashMap<i32, FileState>>> = OnceLock::new();

pub(crate) fn files() -> &'static Mutex<HashMap<i32, FileState>> {
    FILES_INNER.get_or_init(|| Mutex::new(HashMap::new()))
}

// Magic fd base — high enough to not collide with real fds
pub(crate) const MAGIC_FD_BASE: i32 = 0x00534d47_u32 as i32;

pub(crate) fn quiet() -> bool {
    std::env::var("SMUGMAP_QUIET").as_deref() == Ok("1")
}
