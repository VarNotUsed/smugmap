use std::io::Read;

thread_local! {
    static AGENT: ureq::Agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
}

pub fn head_size(url: &str) -> Result<u64, String> {
    AGENT.with(|a| {
        let resp = a.head(url).call().map_err(|e| e.to_string())?;
        resp.header("content-length")
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| "no content-length in HEAD response".into())
    })
}

pub fn fetch_range(url: &str, start: u64, end: u64) -> Result<Vec<u8>, String> {
    AGENT.with(|a| {
        let resp = a
            .get(url)
            .set("Range", &format!("bytes={start}-{end}"))
            .call()
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        Ok(buf)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests require the local HTTP server running on port 8787.
    // Start it with: python3 test/serve.py 8787
    // Then run: cargo test -- --ignored

    #[test]
    #[ignore = "requires local HTTP server on :8787"]
    fn test_head_returns_content_length() {
        let size = head_size("http://127.0.0.1:8787/Cargo.toml").unwrap();
        let actual = std::fs::metadata("Cargo.toml").unwrap().len();
        assert_eq!(size, actual);
    }

    #[test]
    #[ignore = "requires local HTTP server on :8787"]
    fn test_fetch_range_returns_correct_bytes() {
        let bytes = fetch_range("http://127.0.0.1:8787/Cargo.toml", 0, 3).unwrap();
        let actual = &std::fs::read("Cargo.toml").unwrap()[0..4];
        assert_eq!(bytes, actual);
    }
}
