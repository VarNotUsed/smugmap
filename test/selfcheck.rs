use std::fs;
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    let content = b"Hello, smugmap! 0123456789abcdef";
    let test_file = "/tmp/smugmap-test-data.bin";
    let config_file = "/tmp/smugmap-test.json";
    let fake_path = "/tmp/smugmap-test.bin";

    // Write test data
    fs::write(test_file, content).expect("write test file");

    // Write config: *.bin → serve the test file
    let config = format!(
        r#"[{{"pattern":"*.bin","url":"http://127.0.0.1:8787{}"}}]"#,
        test_file
    );
    fs::write(config_file, &config).expect("write config");

    // Start local HTTP server
    let serve_py = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("test/serve.py");
    let mut server = Command::new("python3")
        .arg(&serve_py)
        .arg("8787")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start serve.py");

    sleep(Duration::from_millis(300));

    // Open the fake path — LD_PRELOAD intercepts this
    let mut f = fs::File::open(fake_path).expect("open intercepted path");

    // Read first 5 bytes
    let mut buf = [0u8; 5];
    f.read_exact(&mut buf).expect("read bytes");
    assert_eq!(&buf, b"Hello", "expected 'Hello', got {:?}", &buf);

    // Cleanup
    server.kill().ok();
    server.wait().ok();
    fs::remove_file(config_file).ok();

    println!("selfcheck: PASS");
}
