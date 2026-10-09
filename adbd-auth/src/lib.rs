//! Preloaded into the CGS2's adbd, which calls OpenSSL's `RSA_verify` with mincrypt's arguments.
#![no_std]

use core::ffi::{c_char, c_void};
use core::ptr::null_mut;

const WORDS: usize = 64;
const SHA1_DIGEST_INFO: [u8; 15] = [0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14];

#[repr(C)]
pub struct MincryptKey {
    len: i32,
    n0inv: u32,
    n: [u32; WORDS],
    rr: [u32; WORDS],
    exponent: i32,
}

extern "C" {
    fn BN_lebin2bn(s: *const u8, len: i32, ret: *mut c_void) -> *mut c_void;
    fn BN_new() -> *mut c_void;
    fn BN_set_word(a: *mut c_void, w: u64) -> i32;
    fn BN_free(a: *mut c_void);
    fn RSA_new() -> *mut c_void;
    fn RSA_free(r: *mut c_void);
    fn RSA_set0_key(r: *mut c_void, n: *mut c_void, e: *mut c_void, d: *mut c_void) -> i32;
    fn RSA_public_decrypt(flen: i32, from: *const u8, to: *mut u8, rsa: *mut c_void, padding: i32) -> i32;
    fn unsetenv(name: *const c_char) -> i32;
    fn getauxval(kind: u64) -> *const c_char;
}

const AT_EXECFN: u64 = 31;

/// Keeps the preload out of adbd's children, while launchers like setsid still pass it on.
#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = drop_preload;

extern "C" fn drop_preload() {
    unsafe {
        let exe = getauxval(AT_EXECFN);
        if !exe.is_null() && core::ffi::CStr::from_ptr(exe).to_bytes().ends_with(b"/adbd") {
            unsetenv(c"LD_PRELOAD".as_ptr());
        }
    }
}

/// mincrypt's `RSA_verify(key, signature, len, sha)`: PKCS#1 v1.5 over a 20-byte SHA-1 digest.
#[no_mangle]
pub unsafe extern "C" fn RSA_verify(key: *const MincryptKey, sig: *const u8, len: i32, sha: *const u8) -> i32 {
    let key = &*key;
    if key.len as usize != WORDS || len != (WORDS * 4) as i32 {
        return 0;
    }
    let n = BN_lebin2bn(key.n.as_ptr().cast(), len, null_mut());
    let e = BN_new();
    let rsa = RSA_new();
    if n.is_null() || e.is_null() || rsa.is_null() || BN_set_word(e, key.exponent as u64) != 1 || RSA_set0_key(rsa, n, e, null_mut()) != 1 {
        BN_free(n);
        BN_free(e);
        RSA_free(rsa);
        return 0;
    }
    let mut out = [0u8; WORDS * 4];
    let got = RSA_public_decrypt(len, sig, out.as_mut_ptr(), rsa, 1);
    RSA_free(rsa);
    let digest = core::slice::from_raw_parts(sha, 20);
    (got == 35 && out[..15] == SHA1_DIGEST_INFO && out[15..35] == *digest) as i32
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
