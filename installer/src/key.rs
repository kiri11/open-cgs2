//! The PC's ADB key: `~/.android/adbkey`, shared with Google's adb, made when missing, and its
//! public half in adb's format for the device's /adb_keys.
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, RsaPrivateKey};
use std::path::PathBuf;

const BITS: usize = 2048;

pub fn path() -> PathBuf {
    std::env::var_os("ANDROID_USER_HOME")
        .map(|h| PathBuf::from(h).join("android"))
        .or_else(|| std::env::home_dir().map(|h| h.join(".android")))
        .unwrap_or_default()
        .join("adbkey")
}

/// Loads the key, making one first if there is none, and returns its public half.
pub fn public() -> Result<String, String> {
    let path = path();
    let key = match std::fs::read_to_string(&path) {
        Ok(pem) => RsaPrivateKey::from_pkcs8_pem(&pem).map_err(|e| format!("{}: {e}", path.display()))?,
        Err(_) => {
            let key = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, BITS).map_err(|e| e.to_string())?;
            let pem = key.to_pkcs8_pem(LineEnding::LF).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&path, pem.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
            key
        }
    };
    Ok(encode(key.n(), key.e()))
}

/// Android's RSAPublicKey (libcrypto_utils' android_pubkey_encode), base64, with a comment.
fn encode(n: &BigUint, e: &BigUint) -> String {
    let words = BITS / 32;
    let le_words = |v: &BigUint| {
        let mut b = v.to_bytes_le();
        b.resize(words * 4, 0);
        b
    };
    let n0 = u32::from_le_bytes(le_words(n)[..4].try_into().unwrap());
    let mut inv = n0;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(n0.wrapping_mul(inv)));
    }
    let r = BigUint::from(1u32) << BITS;
    let rr = (&r * &r) % n;
    let mut out = Vec::with_capacity(12 + words * 8);
    out.extend((words as u32).to_le_bytes());
    out.extend(inv.wrapping_neg().to_le_bytes());
    out.extend(le_words(n));
    out.extend(le_words(&rr));
    out.extend((e.to_bytes_le().iter().rev().fold(0u32, |a, b| a << 8 | *b as u32)).to_le_bytes());
    format!("{} open-cgs2", base64(&out))
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let v = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            s.push(if i <= c.len() { T[(v >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_adb() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        let key = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, BITS).unwrap();
        let text = encode(key.n(), key.e());
        let (b64, comment) = text.split_once(' ').unwrap();
        assert_eq!((b64.len(), comment), (700, "open-cgs2"));
        assert!(b64.starts_with("QAAAA"));
    }
}
