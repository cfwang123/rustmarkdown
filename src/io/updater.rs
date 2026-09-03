//! 自更新：查 GitHub Releases → 下载 .7z → 复制自身到数据目录 → 命令行解压覆盖安装目录。
//! 流程对齐 ScreenKit `AppUpdater`；检查间隔与上次检查时间存 settings.json（对齐 SerialTool / ScreenKit）。
//! 网络走本机代理 127.0.0.1:7897（GitHub 为非国内站点），传输层失败再直连一次。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::i18n;
use crate::io::settings;

pub const OWNER: &str = "cfwang123";
pub const REPO: &str = "rustmarkdown";
pub const RELEASES_PAGE: &str = "https://github.com/cfwang123/rustmarkdown/releases";

fn latest_api_url() -> String {
    format!("https://api.github.com/repos/{OWNER}/{REPO}/releases/latest")
}
const UPDATER_EXE: &str = "rustmarkdown_updater.exe";
const MAIN_EXE: &str = "rustmarkdown.exe";
/// 本机代理（对齐其它程序的默认配置）。
const PROXY_ADDR: &str = "http://127.0.0.1:7897";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const CHECK_READ_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(30);
const WAIT_PID_MAX_MS: u64 = 120_000;
const EXTRACT_TIMEOUT_MS: u64 = 600_000;

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub tag: String,
    pub asset_name: String,
    pub download_url: String,
    pub size: u64,
    pub html_url: String,
    pub has_update: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadFail {
    /// 用户取消。
    Cancelled,
    Msg(String),
}

impl From<String> for DownloadFail {
    fn from(s: String) -> Self {
        DownloadFail::Msg(s)
    }
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: Option<String>,
    name: Option<String>,
    html_url: Option<String>,
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: Option<String>,
    browser_download_url: Option<String>,
    size: Option<u64>,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 下载目录（数据目录下 tmp/，装在哪都能写）。
pub fn download_dir() -> PathBuf {
    settings::data_dir().join("tmp")
}

/// 是否到了自动检查时间：days<=0 关闭；从未检查视为到期。
pub fn auto_due(days: i32, last_unix: i64) -> bool {
    if days <= 0 {
        return false;
    }
    if last_unix <= 0 {
        return true;
    }
    let days = days.min(3650) as i64;
    now_unix() - last_unix >= days * 86400
}

/// 命令行入口：解压更新包覆盖安装目录。返回进程退出码。
pub fn is_apply_update_args(args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--apply-update" || a == "--self-update")
}

pub fn run_apply_update(args: &[String]) -> i32 {
    let mut archive: Option<String> = None;
    let mut target: Option<String> = None;
    let mut wait_pid: u32 = 0;
    let mut restart = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--apply-update" | "--self-update" => {
                i += 1;
                if i < args.len() {
                    archive = Some(args[i].clone());
                }
            }
            "--target" => {
                i += 1;
                if i < args.len() {
                    target = Some(args[i].clone());
                }
            }
            "--wait-pid" => {
                i += 1;
                if i < args.len() {
                    wait_pid = args[i].parse().unwrap_or(0);
                }
            }
            "--restart" => restart = true,
            _ => {}
        }
        i += 1;
    }
    let Some(archive) = archive else {
        return fail("缺少 --apply-update <更新包路径>");
    };
    let target = target.unwrap_or_default();
    if target.is_empty() {
        return fail("缺少 --target <安装目录>");
    }
    let r = apply(
        PathBuf::from(archive),
        PathBuf::from(target),
        wait_pid,
        restart,
    );
    match r {
        Ok(()) => 0,
        Err(e) => fail(&e),
    }
}

fn fail(msg: &str) -> i32 {
    let dir = settings::data_dir().join("tmp");
    logline_at(&dir, &format!("FAIL: {msg}"));
    #[cfg(windows)]
    msgbox(i18n::t().update_title, msg);
    eprintln!("{msg}");
    1
}

fn apply(archive: PathBuf, target: PathBuf, wait_pid: u32, restart: bool) -> Result<(), String> {
    if !archive.is_file() {
        return Err(format!("{}: {}", i18n::t().update_missing_pkg, archive.display()));
    }
    if !target.is_dir() {
        return Err(format!("{}: {}", i18n::t().update_missing_target, target.display()));
    }
    logline(&target, &format!(
        "apply-update archive={} target={} wait-pid={} restart={}",
        archive.display(),
        target.display(),
        wait_pid,
        restart
    ));
    if wait_pid > 0 {
        logline(&target, &format!("wait for pid {wait_pid}…"));
        wait_pid_exit(wait_pid);
        logline(&target, "pid exited");
    }

    // 解压到 tmp/update_extract，再覆盖到安装目录（避免半截写入）
    let extract_dir = target.join("tmp").join("update_extract");
    if extract_dir.exists() {
        let _ = std::fs::remove_dir_all(&extract_dir);
    }
    std::fs::create_dir_all(&extract_dir).map_err(|e| e.to_string())?;
    logline(&target, "extract…");
    extract_archive(&archive, &extract_dir)?;
    let payload = resolve_payload(&extract_dir);
    logline(&target, &format!("payload={}", payload.display()));

    logline(&target, "copy overwrite…");
    copy_tree(&payload, &target)?;
    logline(&target, "copy done");

    let main = target.join(MAIN_EXE);
    if !main.is_file() {
        return Err(format!("{}: {}", i18n::t().update_no_main, main.display()));
    }
    let _ = std::fs::remove_dir_all(&extract_dir);

    if restart {
        logline(&target, &format!("restart {}", main.display()));
        Command::new(&main)
            .current_dir(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    logline(&target, "ok");
    Ok(())
}

/// 查询最新 Release；网络失败返回错误。
pub fn check_latest() -> Result<UpdateInfo, String> {
    let body = get_text(&latest_api_url())?;
    parse_release(&body, current_version())
}

/// 调试：--update-check 只查一次最新版本，结果写 update_apply.log。
pub fn cli_check_once() {
    let r = check_latest();
    match r {
        Ok(info) => logline_at(
            &download_dir(),
            &format!(
                "check ok cur={} latest={} tag={} url={} size={}",
                current_version(),
                info.version,
                info.tag,
                info.download_url,
                info.size
            ),
        ),
        Err(e) => logline_at(&download_dir(), &format!("check fail: {e}")),
    }
}

/// 下载更新包到 `dir`，进度 0..=1 发到通道；`cancel` 置位后尽快中止并清理。
/// `repaint` 用于通知 egui 界面刷新进度（下载在后台线程）。
pub fn download(
    info: &UpdateInfo,
    dir: &Path,
    progress: Sender<f32>,
    cancel: Arc<AtomicBool>,
    repaint: Option<egui::Context>,
) -> Result<PathBuf, DownloadFail> {
    let name = {
        let raw = if info.asset_name.is_empty() {
            format!("rustmarkdown_update{}", ext_of(&info.download_url))
        } else {
            info.asset_name.clone()
        };
        sanitize_name(&raw)
    };
    let _ = std::fs::create_dir_all(dir);
    let dest = dir.join(&name);
    let part = dir.join(format!("{name}.part"));
    let _ = std::fs::remove_file(&part);

    let resp = get_response(&info.download_url)?;
    let total: u64 = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut reader = resp.into_reader();
    let mut out = std::fs::File::create(&part).map_err(|e| DownloadFail::from(e.to_string()))?;
    let mut done: u64 = 0;
    let mut buf = [0u8; 128 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Err(DownloadFail::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| DownloadFail::from(e.to_string()))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| DownloadFail::from(e.to_string()))?;
        done += n as u64;
        if total > 0 {
            let _ = progress.send((done as f32 / total as f32).min(1.0));
            if let Some(c) = &repaint {
                c.request_repaint();
            }
        }
    }
    drop(out);
    if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) < 64 {
        let _ = std::fs::remove_file(&part);
        return Err(DownloadFail::from(i18n::t().update_too_small.to_string()));
    }
    if std::fs::rename(&part, &dest).is_err() {
        let _ = std::fs::remove_file(&part);
        return Err(DownloadFail::from(i18n::t().update_save_fail.to_string()));
    }
    Ok(dest)
}

/// 复制主程序到数据目录/tmp 作为更新器并启动它（带 --wait-pid / --restart）。
/// 调用成功后当前进程应尽快退出（等待进程会替我们覆盖安装目录）。
pub fn launch_updater(archive: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let tmp = download_dir();
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let updater = tmp.join(UPDATER_EXE);
    copy_retry(&exe, &updater)?;
    let pid = std::process::id();
    let target = exe
        .parent()
        .ok_or_else(|| i18n::t().update_no_install_dir.to_string())?;
    logline(target, &format!("launch updater: {} wait-pid={}", updater.display(), pid));
    let mut cmd = Command::new(&updater);
    cmd.arg("--apply-update")
        .arg(archive)
        .arg("--target")
        .arg(target)
        .arg("--wait-pid")
        .arg(pid.to_string())
        .arg("--restart")
        .current_dir(&tmp);
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// 字节数显示：B / KB / MB。
pub fn fmt_size(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    if n >= 100 * MB as u64 {
        format!("{:.1} MB", n as f64 / MB)
    } else if n >= KB as u64 {
        format!("{:.0} KB", n as f64 / KB)
    } else {
        format!("{n} B")
    }
}

/// 本地时间 "2026-09-03 12:34"（Windows 用 GetLocalTime；其它平台按 UTC）。
pub fn fmt_local(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    #[cfg(windows)]
    {
        return win_fmt_local(secs);
    }
    #[cfg(not(windows))]
    {
        fmt_utc(secs)
    }
}

#[cfg(windows)]
fn win_fmt_local(secs: i64) -> String {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    use windows_sys::Win32::System::Time::FileTimeToSystemTime;
    // Unix 秒 → FILETIME：1601-01-01 起 100ns 单位。
    let ft100 = secs * 10_000_000 + 116_444_736_000_000_000_i64;
    let ft = FILETIME {
        dwLowDateTime: ft100 as u32,
        dwHighDateTime: (ft100 >> 32) as u32,
    };
    let mut lft = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        FileTimeToLocalFileTime(&ft, &mut lft) != 0 && FileTimeToSystemTime(&lft, &mut st) != 0
    };
    if !ok {
        return fmt_utc(secs);
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute)
}

fn fmt_utc(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (hh, mm) = (rem / 3600, (rem % 3600) / 60);
    // Howard Hinnant civil_from_days：unix 天 → 年月日
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y0 = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y0 + 1 } else { y0 };
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

// ───────── 网络 ─────────

fn agent_with(proxy: bool, read_timeout: Duration) -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(read_timeout);
    if proxy {
        if let Ok(p) = ureq::Proxy::new(PROXY_ADDR) {
            b = b.proxy(p);
        }
    }
    b.build()
}

fn get_call(url: &str, read_timeout: Duration) -> Result<ureq::Response, String> {
    // 先走本机代理（GitHub 需代理），传输层失败再直连；HTTP 状态错误不换通道。
    let mut last = String::new();
    for use_proxy in [true, false] {
        match agent_with(use_proxy, read_timeout)
            .get(url)
            .set("User-Agent", &format!("rustmarkdown-updater/{}", current_version()))
            .set("Accept", "application/vnd.github+json")
            .call()
        {
            Ok(resp) => return Ok(resp),
            Err(ureq::Error::Status(code, _)) => {
                last = format!("HTTP {code}");
                break;
            }
            Err(ureq::Error::Transport(t)) => last = t.to_string(),
        }
    }
    Err(last)
}

fn get_text(url: &String) -> Result<String, String> {
    let resp = get_call(url, CHECK_READ_TIMEOUT)?;
    resp.into_string().map_err(|e| e.to_string())
}

fn get_response(url: &String) -> Result<ureq::Response, DownloadFail> {
    get_call(url, DOWNLOAD_READ_TIMEOUT).map_err(DownloadFail::Msg)
}

// ───────── Release JSON 解析 ─────────

fn parse_release(json: &str, cur: &str) -> Result<UpdateInfo, String> {
    let rel: ReleaseJson =
        serde_json::from_str(json).map_err(|e| format!("{}: {e}", i18n::t().update_parse_fail))?;
    let tag = rel.tag_name.unwrap_or_default();
    let mut ver = norm_ver(&tag);
    if ver.is_empty() {
        ver = rel.name.as_deref().map(norm_ver).unwrap_or_default();
    }
    if ver.is_empty() {
        ver = tag.trim_start_matches(['v', 'V']).to_string();
    }

    // 选资源：优先 rustmarkdown 前缀的 .7z/.zip，其次任意 .7z/.zip。
    let mut best: Option<&AssetJson> = None;
    for a in &rel.assets {
        let Some(name) = a.name.as_deref() else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".7z") || lower.ends_with(".zip")) {
            continue;
        }
        let cur_pref = lower.starts_with("rustmarkdown");
        match best {
            None => best = Some(a),
            Some(b) => {
                let b_pref = b
                    .name
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .starts_with("rustmarkdown");
                if cur_pref && !b_pref {
                    best = Some(a);
                }
            }
        }
    }
    let Some(asset) = best else {
        return Err(i18n::t().update_no_asset.to_string());
    };

    Ok(UpdateInfo {
        version: ver.clone(),
        tag,
        asset_name: asset.name.clone().unwrap_or_default(),
        download_url: asset
            .browser_download_url
            .clone()
            .unwrap_or_default(),
        size: asset.size.unwrap_or(0),
        html_url: rel.html_url.unwrap_or_else(|| RELEASES_PAGE.to_string()),
        has_update: is_newer(&ver, cur),
    })
}

fn norm_ver(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s);
    s.chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect()
}

fn ver_parts(s: &str) -> [u64; 3] {
    let mut parts = [0u64; 3];
    for (i, seg) in s.split('.').take(3).enumerate() {
        parts[i] = seg.parse().unwrap_or(0);
    }
    parts
}

/// remote 是否比 local 新（按 x.y.z 数字比较）。
fn is_newer(remote: &str, local: &str) -> bool {
    let r = ver_parts(remote);
    let l = ver_parts(local);
    (r[0], r[1], r[2]) > (l[0], l[1], l[2])
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if '<' == c || '>' == c || ':' == c || '"' == c || '/' == c || '\\' == c || '|' == c || '?' == c || '*' == c {
                '_'
            } else {
                c
            }
        })
        .collect()
}

fn ext_of(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    match path.rsplit_once('.') {
        Some((_, ext)) if ext.len() <= 8 => format!(".{}", ext.to_ascii_lowercase()),
        _ => ".7z".into(),
    }
}

// ───────── 解压 / 覆盖 ─────────

#[cfg(windows)]
fn wait_pid_exit(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    // SYNCHRONIZE：句柄锁住原始进程对象再等它终止，不受 PID 重用影响（对齐 ScreenKit）。
    const SYNCHRONIZE: u32 = 0x0010_0000;
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            let _ = WaitForSingleObject(h, WAIT_PID_MAX_MS as u32);
            CloseHandle(h);
        }
    }
    // 给文件句柄释放留一点时间
    std::thread::sleep(Duration::from_millis(500));
}

#[cfg(not(windows))]
fn wait_pid_exit(pid: u32) {
    let _ = pid;
    std::thread::sleep(Duration::from_millis(500));
}

fn extract_archive(archive: &Path, dest: &Path) -> Result<(), String> {
    let ext = archive
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "7z" && ext != "zip" {
        return Err(format!("{}: {ext}", i18n::t().update_bad_format));
    }
    let seven = find_7z().ok_or_else(|| i18n::t().update_need_7z.to_string())?;
    let mut child = Command::new(&seven)
        .args([
            "x",
            &archive.display().to_string(),
            &format!("-o{}", dest.display()),
            "-y",
            "-bb0",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(EXTRACT_TIMEOUT_MS);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                if !st.success() {
                    return Err(format!("{} exit={:?}", i18n::t().update_extract_fail, st.code()));
                }
                return Ok(());
            }
            Ok(None) => {}
            Err(e) => return Err(e.to_string()),
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err(i18n::t().update_extract_timeout.to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn find_7z() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("ProgramFiles") {
            cands.push(PathBuf::from(p).join("7-Zip").join("7z.exe"));
        }
        if let Ok(p) = std::env::var("ProgramFiles(x86)") {
            cands.push(PathBuf::from(p).join("7-Zip").join("7z.exe"));
        }
    }
    for p in [
        "C:\\Program Files\\7-Zip\\7z.exe",
        "C:\\Program Files (x86)\\7-Zip\\7z.exe",
        "C:\\bin\\7z.exe",
        "C:\\bin\\7za.exe",
    ] {
        cands.push(PathBuf::from(p));
    }
    for c in cands {
        if c.is_file() {
            return Some(c);
        }
    }
    // PATH 里找真正的 .exe（避开 7z.cmd 包装）
    for name in ["7z.exe", "7za.exe"] {
        #[cfg(windows)]
        {
            if let Ok(out) = Command::new("where.exe").arg(name).output() {
                if out.status.success() {
                    if let Ok(text) = String::from_utf8(out.stdout) {
                        for line in text.lines() {
                            let p = PathBuf::from(line.trim());
                            if p.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false)
                                && p.is_file()
                            {
                                return Some(p);
                            }
                        }
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            if Command::new("which")
                .arg(name)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                return Some(PathBuf::from(name));
            }
        }
    }
    None
}

/// 解压后若只有一层目录，用内层作为 payload，否则用含主程序的那层。
fn resolve_payload(extract: &Path) -> PathBuf {
    if extract.join(MAIN_EXE).is_file() {
        return extract.to_path_buf();
    }
    if let Ok(rd) = std::fs::read_dir(extract) {
        let dirs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        if dirs.len() == 1 && dirs[0].join(MAIN_EXE).is_file() {
            return dirs[0].clone();
        }
        if let Some(found) = find_main_exe(extract) {
            if let Some(parent) = found.parent() {
                return parent.to_path_buf();
            }
        }
    }
    extract.to_path_buf()
}

fn find_main_exe(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p
                    .file_name()
                    .map(|n| n.eq_ignore_ascii_case(MAIN_EXE))
                    .unwrap_or(false)
                {
                    return Some(p);
                }
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(p);
                }
            }
        }
    }
    None
}

/// 递归覆盖复制；跳过用户数据/临时目录（对齐 ScreenKit copytree）。
fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((s, d)) = stack.pop() {
        if !d.exists() {
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        }
        let rd = std::fs::read_dir(&s).map_err(|e| e.to_string())?;
        for ent in rd {
            let ent = ent.map_err(|e| e.to_string())?;
            let p = ent.path();
            let rel = p.strip_prefix(src).unwrap_or(&p);
            if is_skippable(rel) {
                continue;
            }
            let to = d.join(ent.file_name());
            if ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push((ent.path(), to));
            } else {
                copy_retry(&ent.path(), &to)?;
            }
        }
    }
    Ok(())
}

/// 顶层 tmp / target / log / .git 不进更新（临时产物与源码树不覆盖）。
fn is_skippable(rel: &Path) -> bool {
    let top = rel
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(top.as_str(), "tmp" | "target" | "log" | ".git")
}

fn copy_retry(src: &Path, dst: &Path) -> Result<(), String> {
    let mut last: Option<String> = None;
    for i in 0..8u64 {
        match std::fs::copy(src, dst) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last = Some(e.to_string());
                std::thread::sleep(Duration::from_millis(150 + i * 100));
            }
        }
    }
    Err(format!(
        "{}: {} — {}",
        i18n::t().update_copy_fail,
        dst.display(),
        last.unwrap_or_default()
    ))
}

// ───────── 日志 / 提示 ─────────

fn logline(target: &Path, msg: &str) {
    logline_at(&target.join("tmp"), msg);
}

fn logline_at(dir: &Path, msg: &str) {
    let paths = [
        dir.join("update_apply.log"),
        settings::data_dir().join("tmp").join("update_apply.log"),
    ];
    let line = format!("{} {}\r\n", fmt_local(now_unix()), msg);
    for p in paths {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

#[cfg(windows)]
fn msgbox(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let t: Vec<u16> = (title.to_owned() + "\0").encode_utf16().collect();
    let m: Vec<u16> = (text.to_owned() + "\0").encode_utf16().collect();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "tag_name": "v1.0.3",
        "name": "1.0.3",
        "html_url": "https://github.com/cfwang123/rustmarkdown/releases/tag/v1.0.3",
        "assets": [
            {"name": "rustmarkdown_1.0.3.7z", "browser_download_url": "https://github.com/cfwang123/rustmarkdown/releases/download/v1.0.3/rustmarkdown_1.0.3.7z", "size": 9406211},
            {"name": "other.zip", "browser_download_url": "https://x/other.zip", "size": 100}
        ]
    }"#;

    #[test]
    fn parse_picks_preferred_asset() {
        let info = parse_release(SAMPLE, "1.0.2").unwrap();
        assert!(info.has_update);
        assert_eq!(info.version, "1.0.3");
        assert_eq!(info.asset_name, "rustmarkdown_1.0.3.7z");
        assert_eq!(info.size, 9406211);
    }

    #[test]
    fn parse_same_version_no_update() {
        let info = parse_release(SAMPLE, "1.0.3").unwrap();
        assert!(!info.has_update);
    }

    #[test]
    fn parse_rejects_no_archive() {
        let json = r#"{"tag_name":"1.0.3","assets":[]}"#;
        assert!(parse_release(json, "1.0.2").is_err());
    }

    #[test]
    fn version_cmp() {
        assert!(is_newer("1.0.3", "1.0.2"));
        assert!(is_newer("1.1.0", "1.0.9"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.0.2", "1.0.2"));
        assert!(!is_newer("1.0.2", "1.0.3"));
        assert_eq!(norm_ver("v1.2.3"), "1.2.3");
        assert_eq!(norm_ver("V1.2"), "1.2");
    }

    #[test]
    fn due_logic() {
        assert!(!auto_due(0, 0));
        assert!(auto_due(7, 0));
        assert!(auto_due(7, now_unix() - 8 * 86400));
        assert!(!auto_due(7, now_unix() - 86400));
        assert!(!auto_due(-1, 0));
    }

    #[test]
    fn sanitize_and_ext() {
        assert_eq!(sanitize_name("a:b?c.7z"), "a_b_c.7z");
        assert_eq!(ext_of("https://x/y.zip?token=1"), ".zip");
        assert_eq!(ext_of("https://x/y"), ".7z");
    }

    #[test]
    fn unix_to_utc_text() {
        // 1788400722 = 2026-09-03 01:58 UTC
        assert_eq!(fmt_utc(1788400722), "2026-09-03 01:58");
        assert_eq!(fmt_local(0), "");
        assert_eq!(fmt_local(-1), "");
    }

    #[test]
    #[ignore] // 需要网络：真实下载 GitHub Release 包（走代理），验证下载/进度逻辑。
    fn live_download() {
        let info = check_latest().expect("check_latest");
        let dir = std::env::temp_dir().join("rmd-upd-live");
        let _ = std::fs::remove_dir_all(&dir);
        let (tx, rx) = std::sync::mpsc::channel::<f32>();
        let cancel = Arc::new(AtomicBool::new(false));
        let dest = download(&info, &dir, tx, cancel, None).expect("download");
        assert!(dest.is_file());
        let last = rx.try_iter().last().unwrap_or(0.0);
        assert!(last > 0.99, "进度应到 ~100%，实际 {last}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}