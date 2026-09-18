use std::alloc::System;

#[global_allocator]
static A: System = System;

mod config;
mod http;
mod intercept;
mod uffd;
