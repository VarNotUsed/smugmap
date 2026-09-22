use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const EMPTY_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k = hmac(&k, region.as_bytes());
    let k = hmac(&k, b"s3");
    hmac(&k, b"aws4_request")
}

fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '/') {
            out.push(c);
        } else {
            for b in c.to_string().bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn now_datetime() -> (String, String) {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let (y, mo, d) = epoch_to_ymd(s / 86400);
    let hh = (s / 3600) % 24;
    let mm = (s / 60) % 60;
    let ss = s % 60;
    (
        format!("{y:04}{mo:02}{d:02}T{hh:02}{mm:02}{ss:02}Z"),
        format!("{y:04}{mo:02}{d:02}"),
    )
}

fn epoch_to_ymd(z: u64) -> (u64, u64, u64) {
    // Howard Hinnant's civil calendar algorithm
    let z = z as i64 + 719468; // shift Unix epoch (1970-01-01) to civil epoch (0000-03-01)
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

/// Converts `s3://bucket/key` → HTTPS and computes SigV4 headers.
/// For non-s3:// URLs returns `(url, [])` unchanged (presigned URLs pass through).
pub fn prepare(method: &str, url: &str) -> Result<(String, Vec<(String, String)>), String> {
    if !url.starts_with("s3://") {
        return Ok((url.to_string(), vec![]));
    }

    let rest = &url["s3://".len()..];
    let slash = rest.find('/').ok_or("s3:// URL missing key (no /)")?;
    let bucket = &rest[..slash];
    let key = &rest[slash..]; // includes leading /

    let region = std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .unwrap_or_else(|_| "us-east-1".to_string());
    let access_key = std::env::var("AWS_ACCESS_KEY_ID").map_err(|_| "AWS_ACCESS_KEY_ID not set")?;
    let secret_key =
        std::env::var("AWS_SECRET_ACCESS_KEY").map_err(|_| "AWS_SECRET_ACCESS_KEY not set")?;
    let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

    let host = format!("{bucket}.s3.{region}.amazonaws.com");
    let https_url = format!("https://{host}{key}");
    let (datetime, date) = now_datetime();

    let token_header = session_token
        .as_deref()
        .map(|t| format!("x-amz-security-token:{t}\n"))
        .unwrap_or_default();
    let token_signed = if session_token.is_some() {
        ";x-amz-security-token"
    } else {
        ""
    };

    // Canonical headers must be sorted (host < x-amz-content-sha256 < x-amz-date < x-amz-security-token)
    let canonical_headers = format!(
        "host:{host}\nx-amz-content-sha256:{EMPTY_HASH}\nx-amz-date:{datetime}\n{token_header}"
    );
    let signed_headers = format!("host;x-amz-content-sha256;x-amz-date{token_signed}");

    let canonical_request = format!(
        "{method}\n{}\n\n{canonical_headers}\n{signed_headers}\n{EMPTY_HASH}",
        encode_path(key)
    );

    let credential_scope = format!("{date}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{datetime}\n{credential_scope}\n{}",
        sha256(canonical_request.as_bytes())
    );

    let sig = hex(&hmac(
        &signing_key(&secret_key, &date, &region),
        string_to_sign.as_bytes(),
    ));
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={sig}"
    );

    let mut headers = vec![
        ("x-amz-date".to_string(), datetime),
        ("x-amz-content-sha256".to_string(), EMPTY_HASH.to_string()),
        ("Authorization".to_string(), auth),
    ];
    if let Some(token) = session_token {
        headers.push(("x-amz-security-token".to_string(), token));
    }

    Ok((https_url, headers))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_for_https_url() {
        let (url, headers) = prepare("GET", "https://example.com/file.db").unwrap();
        assert_eq!(url, "https://example.com/file.db");
        assert!(headers.is_empty());
    }

    #[test]
    fn s3_url_missing_slash_returns_err() {
        assert!(prepare("GET", "s3://bucket-only").is_err());
    }

    #[test]
    fn epoch_to_ymd_known_dates() {
        assert_eq!(epoch_to_ymd(0), (1970, 1, 1));
        assert_eq!(epoch_to_ymd(365), (1971, 1, 1));
        assert_eq!(epoch_to_ymd(19523), (2023, 6, 15)); // a known date
    }
}
