//! Vim 加密文件支持。
//!
//! - zip（`VimCrypt~01!`）：PKZIP 流密码，12 字节魔数后即密文。见 vim `crypt_zip.c`。
//! - blowfish（`VimCrypt~02!`）：28 字节头 + 64 字节 CFB 缓冲（有重复块缺陷）。
//! - blowfish2（`VimCrypt~03!`）：28 字节头 + 8 字节 CFB。密钥派生 1000 轮 sha256。
//! 保存按原 method / salt / seed 写回。密码错误无法检测，解出来就是乱码。

use blowfish::BlowfishLE;
use cipher::{Block, BlockCipherEncrypt, KeyInit};
use sha2::{Digest, Sha256};

const MAGIC_LEN: usize = 12;
pub const HEADER_LEN: usize = 28;
const MAGIC_01: &[u8; 12] = b"VimCrypt~01!";
const MAGIC_02: &[u8; 12] = b"VimCrypt~02!";
const MAGIC_03: &[u8; 12] = b"VimCrypt~03!";

/// 存在标签里的解密信息（仅内存，不写入会话/设置）。
#[derive(Clone)]
pub struct VimSecret {
    pub password: String,
    /// 1 = zip，2 = blowfish，3 = blowfish2。
    pub method: u8,
    pub salt: [u8; 8],
    pub seed: [u8; 8],
}

/// 识别 Vim 加密头。返回加密方式：1 = zip、2 = blowfish、3 = blowfish2。
pub fn header_method(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < MAGIC_LEN || !bytes.starts_with(b"VimCrypt~") {
        return None;
    }
    if &bytes[0..12] == MAGIC_03 {
        Some(3)
    } else if &bytes[0..12] == MAGIC_02 {
        Some(2)
    } else if &bytes[0..12] == MAGIC_01 {
        Some(1)
    } else {
        None
    }
}

pub fn method_supported(method: u8) -> bool {
    matches!(method, 1 | 2 | 3)
}

/// 用密码解密整个文件。返回明文字节与解密信息（保存写回时用）。
/// 密码错误无法检测（Vim 本身也不检测），解出来就是乱码，由用户重输。
pub fn decrypt(bytes: &[u8], password: &str) -> Result<(Vec<u8>, VimSecret), String> {
    let Some(method) = header_method(bytes) else {
        return Err(crate::i18n::t().vim_not_encrypted.to_string());
    };
    if !method_supported(method) {
        return Err(crate::i18n::t().vim_unsupported_method.to_string());
    }
    if method == 1 {
        let mut out = bytes[MAGIC_LEN..].to_vec();
        zip_crypt(&mut out, password, false);
        return Ok((
            out,
            VimSecret {
                password: password.to_string(),
                method: 1,
                salt: [0; 8],
                seed: [0; 8],
            },
        ));
    }
    if bytes.len() < HEADER_LEN {
        return Err(crate::i18n::t().vim_not_encrypted.to_string());
    }
    let mut salt = [0u8; 8];
    let mut seed = [0u8; 8];
    salt.copy_from_slice(&bytes[12..20]);
    seed.copy_from_slice(&bytes[20..28]);
    let secret = VimSecret {
        password: password.to_string(),
        method,
        salt,
        seed,
    };
    let bf = cipher_of(password, &salt);
    let mut out = bytes[HEADER_LEN..].to_vec();
    if method == 2 {
        vim_cfb(&bf, &seed, &mut out, false, 64);
    } else {
        cfb_decrypt(&bf, &seed, &mut out);
    }
    Ok((out, secret))
}

/// 用原 method / salt / seed 与密码加密明文，产出可直接落盘的完整文件字节。
pub fn encrypt(plain: &[u8], secret: &VimSecret) -> Vec<u8> {
    if secret.method == 1 {
        let mut out = plain.to_vec();
        zip_crypt(&mut out, &secret.password, true);
        let mut file = Vec::with_capacity(MAGIC_LEN + out.len());
        file.extend_from_slice(MAGIC_01);
        file.extend_from_slice(&out);
        return file;
    }
    let bf = cipher_of(&secret.password, &secret.salt);
    let mut out = plain.to_vec();
    if secret.method == 2 {
        vim_cfb(&bf, &secret.seed, &mut out, true, 64);
    } else {
        cfb_encrypt(&bf, &secret.seed, &mut out);
    }
    let magic = if secret.method == 2 {
        MAGIC_02
    } else {
        MAGIC_03
    };
    let mut file = Vec::with_capacity(HEADER_LEN + out.len());
    file.extend_from_slice(magic);
    file.extend_from_slice(&secret.salt);
    file.extend_from_slice(&secret.seed);
    file.extend_from_slice(&out);
    file
}

fn cipher_of(password: &str, salt: &[u8]) -> BlowfishLE {
    let key = derive_key(password, salt);
    BlowfishLE::new_from_slice(&key).expect("blowfish key is 32 bytes")
}

fn zip_crc_tab() -> &'static [u32; 256] {
    static TAB: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TAB.get_or_init(|| {
        let mut t = [0u32; 256];
        for i in 0..256u32 {
            let mut v = i;
            for _ in 0..8 {
                v = (v >> 1) ^ (0xedb88320u32 * (v & 1));
            }
            t[i as usize] = v;
        }
        t
    })
}

fn zip_crc(crc: u32, byte: u8) -> u32 {
    zip_crc_tab()[((crc ^ byte as u32) & 0xff) as usize] ^ (crc >> 8)
}

fn zip_update(keys: &mut [u32; 3], c: u8) {
    keys[0] = zip_crc(keys[0], c);
    keys[1] = keys[1].wrapping_add(keys[0] & 0xff);
    keys[1] = keys[1].wrapping_mul(134775813).wrapping_add(1);
    keys[2] = zip_crc(keys[2], (keys[1] >> 24) as u8);
}

fn zip_prng_byte(keys: &[u32; 3]) -> u8 {
    let temp = (keys[2] as u16 | 2) as u32;
    ((temp.wrapping_mul(temp ^ 1) >> 8) & 0xff) as u8
}

/// PKZIP 流密码。enc=true 用明文更新密钥，false 用解开后的明文更新。
fn zip_crypt(data: &mut [u8], password: &str, enc: bool) {
    let mut keys = [0x12345678u32, 0x23456789, 0x34567890];
    for &b in password.as_bytes() {
        zip_update(&mut keys, b);
    }
    for b in data.iter_mut() {
        let t = zip_prng_byte(&keys);
        if enc {
            let z = *b;
            zip_update(&mut keys, z);
            *b = t ^ z;
        } else {
            let p = *b ^ t;
            *b = p;
            zip_update(&mut keys, p);
        }
    }
}

/// blowfish（method 2）用 64 字节 CFB 缓冲；与 vim `blowfish.c` 一致。
fn vim_cfb(bf: &BlowfishLE, seed: &[u8; 8], data: &mut [u8], enc: bool, cfb_len: usize) {
    let mut buf = vec![0u8; cfb_len];
    for (i, slot) in buf.iter_mut().enumerate() {
        *slot ^= seed[i % 8];
    }
    let mut rand_off = 0usize;
    let mut upd_off = 0usize;
    for b in data.iter_mut() {
        if rand_off & 7 == 0 {
            let mut block = Block::<BlowfishLE>::default();
            block.copy_from_slice(&buf[rand_off..rand_off + 8]);
            bf.encrypt_block(&mut block);
            buf[rand_off..rand_off + 8].copy_from_slice(&block);
        }
        let t = buf[rand_off];
        rand_off += 1;
        if rand_off == cfb_len {
            rand_off = 0;
        }
        let plain = if enc {
            let p = *b;
            *b = t ^ p;
            p
        } else {
            let p = *b ^ t;
            *b = p;
            p
        };
        buf[upd_off] ^= plain;
        upd_off += 1;
        if upd_off == cfb_len {
            upd_off = 0;
        }
    }
}

/// Blowfish-CFB（8 字节反馈）。
/// 反馈值始终是密文：加密时取输出，解密时取输入。
fn cfb_encrypt(bf: &BlowfishLE, seed: &[u8; 8], data: &mut [u8]) {
    cfb_mode(bf, seed, data, true);
}

fn cfb_decrypt(bf: &BlowfishLE, seed: &[u8; 8], data: &mut [u8]) {
    cfb_mode(bf, seed, data, false);
}

fn cfb_mode(bf: &BlowfishLE, seed: &[u8; 8], data: &mut [u8], enc: bool) {
    let mut feed = *seed;
    for chunk in data.chunks_mut(8) {
        let mut block = Block::<BlowfishLE>::default();
        block.copy_from_slice(&feed);
        bf.encrypt_block(&mut block);
        let n = chunk.len();
        for (i, b) in chunk.iter_mut().enumerate() {
            let c = if enc {
                let c = *b ^ block[i];
                feed[i] = c;
                c
            } else {
                feed[i] = *b;
                *b ^ block[i]
            };
            *b = c;
        }
        if n < 8 {
            break;
        }
    }
}

/// 1000 轮 hex 链式 sha256，最终取 32 字节原始摘要作密钥。
fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut pw: Vec<u8> = password.as_bytes().to_vec();
    for _ in 0..1000 {
        pw = hex_lower(&sha256_cat(&pw, salt));
    }
    sha256_cat(&pw, salt)
}

fn sha256_cat(a: &[u8], b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(a);
    h.update(b);
    h.finalize().into()
}

fn hex_lower(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &x in bytes {
        out.push(HEX[(x >> 4) as usize]);
        out.push(HEX[(x & 0xf) as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_detect() {
        let mk = |magic: &[u8; 12]| -> Vec<u8> {
            let mut v = magic.to_vec();
            v.extend_from_slice(&[0u8; 16]);
            v
        };
        assert_eq!(header_method(&mk(MAGIC_03)), Some(3));
        assert_eq!(header_method(&mk(MAGIC_02)), Some(2));
        assert_eq!(header_method(&mk(MAGIC_01)), Some(1));
        assert_eq!(header_method(MAGIC_01), Some(1));
        assert_eq!(header_method(b"VimCrypt~03!"), Some(3));
        assert_eq!(header_method(b"VimCrypt~04!"), None);
        assert_eq!(header_method(b"hello"), None);
    }

    #[test]
    fn roundtrip_keeps_length() {
        let secret = VimSecret {
            password: "pw中文".into(),
            method: 3,
            salt: *b"12345678",
            seed: *b"abcdef98",
        };
        for len in [0usize, 1, 7, 8, 9, 64, 1000] {
            let plain: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            let blob = encrypt(&plain, &secret);
            assert_eq!(blob.len(), HEADER_LEN + len);
            let (back, s2) = decrypt(&blob, &secret.password).unwrap();
            assert_eq!(back, plain);
            assert_eq!(s2.salt, secret.salt);
            assert_eq!(s2.seed, secret.seed);
        }
    }

    #[test]
    fn wrong_password_differs() {
        let secret = VimSecret {
            password: "right".into(),
            method: 3,
            salt: *b"salt1234",
            seed: *b"seed4321",
        };
        let blob = encrypt(b"top secret", &secret);
        let (wrong, _) = decrypt(&blob, "rong").unwrap();
        assert_ne!(wrong, b"top secret".to_vec());
    }

    #[test]
    fn derive_key_stable() {
        let a = derive_key("test", b"salt");
        let b = derive_key("test", b"salt");
        assert_eq!(a, b);
        assert_ne!(a, derive_key("test", b"salt2"));
    }

    #[test]
    fn zip_roundtrip() {
        let secret = VimSecret {
            password: "zip中文".into(),
            method: 1,
            salt: [0; 8],
            seed: [0; 8],
        };
        for len in [0usize, 1, 7, 8, 9, 64, 1000] {
            let plain: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            let blob = encrypt(&plain, &secret);
            assert_eq!(&blob[0..12], MAGIC_01);
            assert_eq!(blob.len(), MAGIC_LEN + len);
            let (back, s2) = decrypt(&blob, &secret.password).unwrap();
            assert_eq!(back, plain);
            assert_eq!(s2.method, 1);
        }
    }

    /// 对照 crypt_zip.c 的 zip 向量：明文 "# zip test\r\nhello 中文\r\n"，密码 zippass。
    const ZIP_BLOB_B64: &str = "VmltQ3J5cHR+MDEhjC6kJOtrOYYGp9YV+qGtR44bfIuDRv4kGaY=";
    const ZIP_PLAIN_B64: &str = "IyB6aXAgdGVzdA0KaGVsbG8g5Lit5paHDQo=";

    #[test]
    fn zip_vector() {
        let blob = base64_decode(ZIP_BLOB_B64);
        let (plain, secret) = decrypt(&blob, "zippass").unwrap();
        assert_eq!(plain, base64_decode(ZIP_PLAIN_B64));
        assert_eq!(secret.method, 1);
        let re = encrypt(&plain, &secret);
        let (back, _) = decrypt(&re, "zippass").unwrap();
        assert_eq!(back, plain);
    }

    #[test]
    fn blowfish1_roundtrip() {
        let secret = VimSecret {
            password: "bf1pw".into(),
            method: 2,
            salt: *b"salt1234",
            seed: *b"seed4321",
        };
        let plain: Vec<u8> = (0..200).map(|i| (i * 17 % 251) as u8).collect();
        let blob = encrypt(&plain, &secret);
        assert_eq!(&blob[0..12], MAGIC_02);
        let (back, s2) = decrypt(&blob, "bf1pw").unwrap();
        assert_eq!(back, plain);
        assert_eq!(s2.method, 2);
    }

    /// 真 vim 9.1 生成的 blowfish2 文件兼容性向量。
    /// 明文（UTF-8 带 BOM、CRLF）："# 加密测试\r\n\r\n中文 secret 123\r\n- item\r\n"
    const VIM91_BLOB_B64: &str = "VmltQ3J5cHR+MDMh2s0Vmpn/cnIPb/Lhz484y+kvQo6Sb7aA9k0K8o1AXAAu2vD4TDaK+YB5W1Cdh29IcRJGeogWYQiS25oHFFs7BQ==";
    const VIM91_PLAIN_B64: &str =
        "77u/IyDliqDlr4bmtYvor5UNCg0K5Lit5paHIHNlY3JldCAxMjMNCi0gaXRlbQ0K";

    #[test]
    fn vim91_blowfish2_vector() {
        let blob = base64_decode(VIM91_BLOB_B64);
        let (plain, secret) = decrypt(&blob, "vimpass123").unwrap();
        let expected = base64_decode(VIM91_PLAIN_B64);
        assert_eq!(plain, expected);
        // 用相同 salt/seed 写回后与本实现互解一致（vim 端格式不变）。
        let re = encrypt(&plain, &secret);
        let (back, _) = decrypt(&re, "vimpass123").unwrap();
        assert_eq!(back, expected);
    }

    fn base64_decode(s: &str) -> Vec<u8> {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut buf = 0u32;
        let mut bits = 0u32;
        for &c in s.as_bytes() {
            if c == b'=' {
                break;
            }
            let v = T.iter().position(|&t| t == c).unwrap() as u32;
            buf = (buf << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
            }
        }
        out
    }
}
