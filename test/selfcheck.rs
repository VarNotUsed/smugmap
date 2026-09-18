// Integration test binary. Setup (config, server) is done by the Makefile before
// this process starts — LD_PRELOAD hooks fire before main(), so config must exist first.
use std::io::Read;

fn main() {
    let mut f = std::fs::File::open("/tmp/smugmap-test.bin")
        .expect("open failed — is SMUGMAP_CONFIG set and LD_PRELOAD active?");

    let mut buf = [0u8; 5];
    f.read_exact(&mut buf).expect("read bytes");
    assert_eq!(&buf, b"Hello", "expected 'Hello', got {:?}", &buf);

    println!("selfcheck: PASS");
}
