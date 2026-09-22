#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, OnceLock};
    use userfaultfd::{Event, Uffd, UffdBuilder};

    const PAGE_SIZE: usize = 4096;

    struct Region {
        url: String,
        readahead: usize,
        len: usize,
    }

    // BTreeMap so we can find the region containing a fault address efficiently
    static REGIONS: Mutex<BTreeMap<usize, Region>> = Mutex::new(BTreeMap::new());

    // Uffd wraps a RawFd (i32) — safe to send across threads
    struct SendUffd(Uffd);
    // manual Send because Uffd holds a RawFd with no raw-pointer aliasing
    unsafe impl Send for SendUffd {}
    unsafe impl Sync for SendUffd {}

    static UFFD: OnceLock<SendUffd> = OnceLock::new();

    fn uffd() -> &'static Uffd {
        &UFFD
            .get_or_init(|| {
                let uffd = UffdBuilder::new()
                    .close_on_exec(true)
                    .non_blocking(false)
                    .create()
                    .expect("[smugmap] failed to create userfaultfd");

                std::thread::Builder::new()
                    .name("smugmap-uffd".into())
                    .spawn(fault_loop)
                    .expect("[smugmap] failed to spawn uffd thread");

                SendUffd(uffd)
            })
            .0
    }

    pub fn register(
        ptr: *mut libc::c_void,
        len: usize,
        url: String,
        readahead: usize,
    ) -> Result<(), String> {
        let uffd = uffd();
        unsafe {
            uffd.register(ptr, len).map_err(|e| e.to_string())?;
        }
        REGIONS.lock().unwrap().insert(
            ptr as usize,
            Region {
                url,
                readahead,
                len,
            },
        );
        Ok(())
    }

    pub fn unregister(ptr: *mut libc::c_void, len: usize) {
        // Only unregister if uffd was already initialized — avoids eager init on munmap
        if let Some(u) = UFFD.get() {
            REGIONS.lock().unwrap().remove(&(ptr as usize));
            let _ = unsafe { u.0.unregister(ptr, len) };
        }
    }

    fn fault_loop() {
        loop {
            match uffd().read_event() {
                Ok(Some(Event::Pagefault { addr, .. })) => handle_fault(addr as usize),
                Ok(Some(_)) | Ok(None) => continue,
                Err(e) => {
                    if !crate::quiet() {
                        eprintln!("[smugmap] uffd read error: {e}");
                    }
                    continue;
                }
            }
        }
    }

    fn handle_fault(fault_addr: usize) {
        let page_addr = fault_addr & !(PAGE_SIZE - 1);

        let (base_addr, url, readahead) = {
            let regions = REGIONS.lock().unwrap();
            let Some((&base, region)) = regions.range(..=page_addr).next_back() else {
                if !crate::quiet() {
                    eprintln!("[smugmap] fault at {page_addr:#x}: no registered region");
                }
                return;
            };
            if page_addr >= base + region.len {
                if !crate::quiet() {
                    eprintln!("[smugmap] fault at {page_addr:#x}: outside region bounds");
                }
                return;
            }
            (base, region.url.clone(), region.readahead)
        };

        let pages_to_fetch = readahead + 1;

        for i in 0..pages_to_fetch {
            let target_page = page_addr + i * PAGE_SIZE;
            let offset = (target_page - base_addr) as u64;
            let end = offset + PAGE_SIZE as u64 - 1;

            match crate::http::fetch_range(&url, offset, end) {
                Ok(data) => {
                    let mut page = [0u8; PAGE_SIZE];
                    let n = data.len().min(PAGE_SIZE);
                    page[..n].copy_from_slice(&data[..n]);

                    unsafe {
                        let result = uffd().copy(
                            page.as_ptr() as *const libc::c_void,
                            target_page as *mut libc::c_void,
                            PAGE_SIZE,
                            true,
                        );
                        if i == 0 {
                            if let Err(e) = result {
                                if !crate::quiet() {
                                    eprintln!("[smugmap] UFFDIO_COPY failed: {e}");
                                }
                            }
                        }
                        // readahead pages: best-effort, errors already ignored via let _
                    }
                }
                Err(e) => {
                    if !crate::quiet() {
                        eprintln!("[smugmap] fetch failed at offset {offset}: {e}");
                    }
                    unsafe {
                        libc::kill(libc::getpid(), libc::SIGBUS);
                    }
                    return;
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::{register, unregister};

#[cfg(not(target_os = "linux"))]
pub fn register(
    _ptr: *mut libc::c_void,
    _len: usize,
    _url: String,
    _readahead: usize,
) -> Result<(), String> {
    Err("userfaultfd not available on this platform".into())
}

#[cfg(not(target_os = "linux"))]
pub fn unregister(_ptr: *mut libc::c_void, _len: usize) {}
