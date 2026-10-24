//! HTTP(S) GET through the device's curl, which has OpenSSL and CA certificates; the panel itself
//! has no TLS. Used for the weather and by components through `GET /fetch?url=`.
use std::process::Command;

pub fn get(url: &str) -> Result<Vec<u8>, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only http and https URLs".into());
    }
    let out = Command::new("curl").args(["-sSfL", "--max-time", "15", "--", url]).output().map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Decodes a URL query value (`%XX` escapes and `+`).
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or(()) {
                Ok(v) => {
                    out.push(v);
                    i += 3;
                    continue;
                }
                Err(_) => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_query_values() {
        assert_eq!(decode("https%3A%2F%2Fexample.com%2Fa%3Fb%3D1+2"), "https://example.com/a?b=1 2");
        assert_eq!(decode("100%"), "100%");
        assert!(get("file:///etc/passwd").is_err());
    }
}
