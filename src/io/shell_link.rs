use std::path::{Path, PathBuf};

/// 若为 .lnk 则解析到目标路径；否则原样返回。目标不存在时仍返回解析结果。
pub fn resolve(path: &Path) -> PathBuf {
    if !is_lnk(path) {
        return path.to_path_buf();
    }
    #[cfg(windows)]
    if let Some(t) = win::resolve_com(path) {
        if !t.as_os_str().is_empty() {
            return strip_extended(t.canonicalize().unwrap_or(t));
        }
    }
    match resolve_lnk_binary(path) {
        Some(t) if !t.as_os_str().is_empty() => t,
        _ => path.to_path_buf(),
    }
}

pub fn is_lnk(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
}

fn resolve_lnk_binary(path: &Path) -> Option<PathBuf> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 0x4C {
        return None;
    }
    if u32::from_le_bytes(data[0..4].try_into().ok()?) != 0x4C {
        return None;
    }
    let flags = u32::from_le_bytes(data[0x14..0x18].try_into().ok()?);
    let is_unicode = flags & 0x80 != 0;
    let mut pos = 0x4Cusize;
    let mut from_idlist = None;
    if flags & 0x1 != 0 {
        let id_size = read_u16(&data, pos)? as usize;
        #[cfg(windows)]
        if pos + 2 < data.len() {
            from_idlist = win::path_from_pidl(data[pos + 2..].as_ptr().cast());
        }
        pos = pos.checked_add(2)?.checked_add(id_size)?;
    }
    let mut from_info = None;
    if flags & 0x2 != 0 {
        if pos + 4 > data.len() {
            return None;
        }
        let info_size = read_u32(&data, pos)? as usize;
        if info_size >= 0x1C && pos + info_size <= data.len() {
            from_info = parse_link_info(&data[pos..pos + info_size]).map(PathBuf::from);
        }
        pos = pos.checked_add(info_size.max(4))?;
    }
    let strings = parse_string_data(&data, pos, flags, is_unicode);
    let mut target = pick_existing(from_idlist, from_info);
    if target
        .as_ref()
        .map(|p| p.as_os_str().is_empty())
        .unwrap_or(true)
    {
        if let Some(rel) = strings.relative {
            if let Some(parent) = path.parent() {
                target = Some(parent.join(rel));
            }
        }
    }
    if let Some(env) = strings.env {
        let envp = PathBuf::from(expand_env(&env));
        let local_ok = target.as_ref().is_some_and(|t| t.exists());
        if !local_ok {
            target = Some(envp);
        }
    }
    let p = target?;
    if p.as_os_str().is_empty() {
        return None;
    }
    Some(strip_extended(p.canonicalize().unwrap_or(p)))
}

fn pick_existing(a: Option<PathBuf>, b: Option<PathBuf>) -> Option<PathBuf> {
    match (&a, &b) {
        (Some(x), _) if x.exists() => a,
        (_, Some(y)) if y.exists() => b,
        (Some(_), _) => a,
        _ => b,
    }
}

fn strip_extended(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p
    }
}

struct LnkStrings {
    relative: Option<String>,
    env: Option<String>,
}

fn parse_link_info(info: &[u8]) -> Option<String> {
    if info.len() < 0x1C {
        return None;
    }
    let header_size = read_u32(info, 4)? as usize;
    let info_flags = read_u32(info, 8)?;
    if info_flags & 1 == 0 {
        return None;
    }
    let ansi_off = read_u32(info, 16)? as usize;
    let suffix_off = read_u32(info, 0x18)? as usize;
    if header_size >= 0x24 && info.len() >= 0x24 {
        let uni_off = read_u32(info, 0x1C)? as usize;
        let uni_suf = read_u32(info, 0x20)? as usize;
        let base = if uni_off > 0 && uni_off < info.len() {
            read_wsz(info, uni_off)
        } else {
            None
        };
        let suf = if uni_suf > 0 && uni_suf < info.len() {
            read_wsz(info, uni_suf).unwrap_or_default()
        } else {
            String::new()
        };
        if let Some(b) = base {
            if !b.is_empty() {
                return Some(concat_path(&b, &suf));
            }
        }
    }
    let base = if ansi_off > 0 && ansi_off < info.len() {
        read_sz(info, ansi_off)?
    } else {
        return None;
    };
    let suf = if suffix_off > 0 && suffix_off < info.len() {
        read_sz(info, suffix_off).unwrap_or_default()
    } else {
        String::new()
    };
    Some(concat_path(&base, &suf))
}

fn concat_path(base: &str, suffix: &str) -> String {
    let base = base.trim_end_matches(['\\', '/']);
    if suffix.is_empty() {
        return base.to_string();
    }
    let suffix = suffix.trim_start_matches(['\\', '/']);
    format!("{base}\\{suffix}")
}

fn parse_string_data(data: &[u8], mut pos: usize, flags: u32, is_unicode: bool) -> LnkStrings {
    let mut relative = None;
    let order = [
        (0x4, false),  // HasName
        (0x8, true),   // HasRelativePath
        (0x10, false), // HasWorkingDir
        (0x20, false), // HasArguments
        (0x40, false), // HasIconLocation
    ];
    for (bit, keep) in order {
        if flags & bit == 0 {
            continue;
        }
        let Some((s, next)) = read_counted_string(data, pos, is_unicode) else {
            break;
        };
        pos = next;
        if keep {
            relative = Some(s);
        }
    }
    let env = parse_extra_env(data, pos);
    LnkStrings { relative, env }
}

fn parse_extra_env(data: &[u8], mut pos: usize) -> Option<String> {
    while pos + 8 <= data.len() {
        let size = read_u32(data, pos)? as usize;
        if size < 8 || pos + size > data.len() {
            break;
        }
        let sig = read_u32(data, pos + 4)?;
        if sig == 0xA0000001 && size >= 0x314 {
            let uni = read_wsz(data, pos + 8 + 260);
            if let Some(s) = uni {
                if !s.is_empty() {
                    return Some(s);
                }
            }
            if let Some(s) = read_sz(data, pos + 8) {
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        if size == 0 {
            break;
        }
        pos += size;
    }
    None
}

fn read_counted_string(data: &[u8], pos: usize, unicode: bool) -> Option<(String, usize)> {
    let count = read_u16(data, pos)? as usize;
    let start = pos + 2;
    if unicode {
        let bytes = count.checked_mul(2)?;
        if start + bytes > data.len() {
            return None;
        }
        let s = utf16_from_bytes(&data[start..start + bytes]);
        Some((s, start + bytes))
    } else {
        if start + count > data.len() {
            return None;
        }
        let raw = &data[start..start + count];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Some((decode_ansi(&raw[..end]), start + count))
    }
}

fn read_sz(data: &[u8], off: usize) -> Option<String> {
    if off >= data.len() {
        return None;
    }
    let end = data[off..]
        .iter()
        .position(|&b| b == 0)
        .map(|p| off + p)
        .unwrap_or(data.len());
    Some(decode_ansi(&data[off..end]))
}

fn decode_ansi(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        if let Some(s) = win::ansi_to_string(bytes) {
            return s;
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn read_wsz(data: &[u8], off: usize) -> Option<String> {
    if off + 1 >= data.len() {
        return None;
    }
    let mut units = Vec::new();
    let mut i = off;
    while i + 1 < data.len() {
        let u = u16::from_le_bytes([data[i], data[i + 1]]);
        i += 2;
        if u == 0 {
            break;
        }
        units.push(u);
    }
    Some(String::from_utf16_lossy(&units))
}

fn utf16_from_bytes(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let u = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        i += 2;
        if u == 0 {
            break;
        }
        units.push(u);
    }
    String::from_utf16_lossy(&units)
}

fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '%') {
                let name: String = chars[i + 1..i + 1 + rel].iter().collect();
                if !name.is_empty() {
                    if let Ok(val) = std::env::var(&name) {
                        out.push_str(&val);
                        i += 2 + rel;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    data.get(off..off + 2)?
        .try_into()
        .ok()
        .map(u16::from_le_bytes)
}

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    data.get(off..off + 4)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};
    use std::ptr;

    #[repr(C)]
    struct Guid {
        d1: u32,
        d2: u16,
        d3: u16,
        d4: [u8; 8],
    }

    const CLSID_SHELL_LINK: Guid = Guid {
        d1: 0x0002_1401,
        d2: 0,
        d3: 0,
        d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    const IID_ISHELL_LINK_W: Guid = Guid {
        d1: 0x0002_14F9,
        d2: 0,
        d3: 0,
        d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    const IID_IPERSIST_FILE: Guid = Guid {
        d1: 0x0000_010B,
        d2: 0,
        d3: 0,
        d4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
    };

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(pv: *mut core::ffi::c_void, dw: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(
            clsid: *const Guid,
            outer: *mut core::ffi::c_void,
            ctx: u32,
            iid: *const Guid,
            ppv: *mut *mut core::ffi::c_void,
        ) -> i32;
        fn CoTaskMemFree(pv: *mut core::ffi::c_void);
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHGetPathFromIDListW(pidl: *const core::ffi::c_void, psz: *mut u16) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn MultiByteToWideChar(
            cp: u32,
            flags: u32,
            src: *const u8,
            srclen: i32,
            dst: *mut u16,
            dstlen: i32,
        ) -> i32;
    }

    #[cfg(test)]
    #[link(name = "kernel32")]
    extern "system" {
        fn GetACP() -> u32;
    }

    struct ComGuard {
        uninit: bool,
    }

    impl ComGuard {
        fn enter() -> Self {
            let hr = unsafe { CoInitializeEx(ptr::null_mut(), 0x2) };
            Self { uninit: hr >= 0 }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.uninit {
                unsafe { CoUninitialize() };
            }
        }
    }

    unsafe fn vcall<T>(this: *mut core::ffi::c_void, idx: usize) -> T {
        let vtbl = *(this as *const *const usize);
        std::mem::transmute_copy(&*vtbl.add(idx))
    }

    unsafe fn release(this: *mut core::ffi::c_void) {
        if this.is_null() {
            return;
        }
        let f: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 = vcall(this, 2);
        f(this);
    }

    unsafe fn query_interface(
        this: *mut core::ffi::c_void,
        iid: *const Guid,
    ) -> Option<*mut core::ffi::c_void> {
        let f: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *const Guid,
            *mut *mut core::ffi::c_void,
        ) -> i32 = vcall(this, 0);
        let mut out = ptr::null_mut();
        if f(this, iid, &mut out) >= 0 && !out.is_null() {
            Some(out)
        } else {
            None
        }
    }

    fn to_wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn from_wide(buf: &[u16]) -> PathBuf {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        PathBuf::from(OsString::from_wide(&buf[..n]))
    }

    pub fn ansi_to_string(bytes: &[u8]) -> Option<String> {
        if bytes.is_empty() {
            return Some(String::new());
        }
        unsafe {
            let n =
                MultiByteToWideChar(0, 0, bytes.as_ptr(), bytes.len() as i32, ptr::null_mut(), 0);
            if n <= 0 {
                return None;
            }
            let mut buf = vec![0u16; n as usize];
            let n = MultiByteToWideChar(
                0,
                0,
                bytes.as_ptr(),
                bytes.len() as i32,
                buf.as_mut_ptr(),
                n,
            );
            if n <= 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..n as usize]))
        }
    }

    pub fn path_from_pidl(pidl: *const core::ffi::c_void) -> Option<PathBuf> {
        if pidl.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 32768];
        let ok = unsafe { SHGetPathFromIDListW(pidl, buf.as_mut_ptr()) };
        if ok == 0 || buf[0] == 0 {
            return None;
        }
        Some(from_wide(&buf))
    }

    pub fn resolve_com(path: &Path) -> Option<PathBuf> {
        let _com = ComGuard::enter();
        let mut shell = ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_SHELL_LINK,
                ptr::null_mut(),
                1,
                &IID_ISHELL_LINK_W,
                &mut shell,
            )
        };
        if hr < 0 || shell.is_null() {
            return None;
        }
        let result = unsafe { resolve_loaded(shell, path) };
        unsafe { release(shell) };
        result
    }

    unsafe fn resolve_loaded(shell: *mut core::ffi::c_void, path: &Path) -> Option<PathBuf> {
        let persist = query_interface(shell, &IID_IPERSIST_FILE)?;
        let wide = to_wide(path);
        type LoadFn = unsafe extern "system" fn(*mut core::ffi::c_void, *const u16, u32) -> i32;
        let load: LoadFn = vcall(persist, 5);
        let hr = load(persist, wide.as_ptr(), 0);
        release(persist);
        if hr < 0 {
            return None;
        }
        let mut buf = vec![0u16; 32768];
        type GetPathFn = unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *mut u16,
            i32,
            *mut core::ffi::c_void,
            u32,
        ) -> i32;
        let get_path: GetPathFn = vcall(shell, 3);
        let _ = get_path(
            shell,
            buf.as_mut_ptr(),
            buf.len() as i32,
            ptr::null_mut(),
            0,
        );
        if buf[0] != 0 {
            return Some(from_wide(&buf));
        }
        let mut pidl = ptr::null_mut();
        type GetIdListFn =
            unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut core::ffi::c_void) -> i32;
        let get_idlist: GetIdListFn = vcall(shell, 4);
        if get_idlist(shell, &mut pidl) >= 0 && !pidl.is_null() {
            let p = path_from_pidl(pidl);
            CoTaskMemFree(pidl);
            return p;
        }
        None
    }

    #[cfg(test)]
    pub fn acp() -> u32 {
        unsafe { GetACP() }
    }

    #[cfg(test)]
    pub fn create_lnk(lnk: &Path, target: &Path) -> bool {
        let _com = ComGuard::enter();
        let mut shell = ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_SHELL_LINK,
                ptr::null_mut(),
                1,
                &IID_ISHELL_LINK_W,
                &mut shell,
            )
        };
        if hr < 0 || shell.is_null() {
            return false;
        }
        let ok = unsafe { save_lnk(shell, lnk, target) };
        unsafe { release(shell) };
        ok
    }

    #[cfg(test)]
    unsafe fn save_lnk(shell: *mut core::ffi::c_void, lnk: &Path, target: &Path) -> bool {
        type SetPathFn = unsafe extern "system" fn(*mut core::ffi::c_void, *const u16) -> i32;
        let set_path: SetPathFn = vcall(shell, 20);
        let target_w = to_wide(target);
        if set_path(shell, target_w.as_ptr()) < 0 {
            return false;
        }
        let Some(persist) = query_interface(shell, &IID_IPERSIST_FILE) else {
            return false;
        };
        type SaveFn = unsafe extern "system" fn(*mut core::ffi::c_void, *const u16, i32) -> i32;
        let save: SaveFn = vcall(persist, 6);
        let lnk_w = to_wide(lnk);
        let hr = save(persist, lnk_w.as_ptr(), 1);
        release(persist);
        hr >= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn lnk_extension() {
        assert!(is_lnk(Path::new(r"C:\a.lnk")));
        assert!(is_lnk(Path::new("notes.LNK")));
        assert!(!is_lnk(Path::new("notes.md")));
    }

    #[test]
    fn resolve_passthrough() {
        let p = Path::new("notes.md");
        assert_eq!(resolve(p), p);
    }

    #[cfg(windows)]
    #[test]
    fn ansi_gbk_when_acp_936() {
        if win::acp() != 936 {
            return;
        }
        assert_eq!(decode_ansi(&[0xD6, 0xD0]), "中");
    }

    #[cfg(windows)]
    #[test]
    fn roundtrip_chinese_lnk() {
        let dir = std::env::temp_dir().join("rustmarkdown_lnk_zh");
        let _ = std::fs::create_dir_all(&dir);
        let md = dir.join("中文测试.md");
        std::fs::write(&md, "# hi\n").unwrap();
        let lnk = dir.join("中文测试.lnk");
        assert!(win::create_lnk(&lnk, &md), "create .lnk");
        let got = resolve(&lnk);
        let want = md.canonicalize().unwrap_or(md);
        let got_s = strip_extended(got).to_string_lossy().to_lowercase();
        let want_s = strip_extended(want).to_string_lossy().to_lowercase();
        assert_eq!(got_s, want_s);
        let _ = std::fs::remove_file(&lnk);
        let _ = std::fs::remove_file(dir.join("中文测试.md"));
        let _ = std::fs::remove_dir(&dir);
    }
}
