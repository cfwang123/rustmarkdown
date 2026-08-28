//! 单实例：已有进程时把路径转过去，在已开窗口用新标签打开（对齐 docview）。
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::io::settings;

const CONNECT_RETRY: u32 = 40;
const CONNECT_WAIT_MS: u64 = 80;

static UI_CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

#[derive(Serialize, Deserialize)]
struct InstanceInfo {
    pid: u32,
    port: u16,
}

pub struct Incoming {
    rx: Receiver<Vec<PathBuf>>,
    _guard: Option<Guard>,
}

struct Guard {
    _lock: File,
    stop: Arc<AtomicBool>,
    port: u16,
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(80));
        let _ = std::fs::remove_file(info_path());
        if let Ok(mut g) = UI_CTX.lock() {
            *g = None;
        }
    }
}

impl Incoming {
    pub fn poll(&self) -> Vec<Vec<PathBuf>> {
        let mut out = Vec::new();
        while let Ok(batch) = self.rx.try_recv() {
            out.push(batch);
        }
        out
    }
}

pub fn attach_ui(ctx: &egui::Context) {
    if let Ok(mut g) = UI_CTX.lock() {
        *g = Some(ctx.clone());
    }
}

/// 成为主实例并开始监听；若已有实例则转发路径并返回 `None`（调用方应退出）。
pub fn claim(paths: &[PathBuf]) -> Option<Incoming> {
    if let Some(lock) = try_lock() {
        return Some(start_primary(lock));
    }
    if send_open(paths) {
        return None;
    }
    let _ = std::fs::remove_file(lock_path());
    if let Some(lock) = try_lock() {
        return Some(start_primary(lock));
    }
    let (tx, rx) = mpsc::channel();
    drop(tx);
    Some(Incoming { rx, _guard: None })
}

fn start_primary(lock: File) -> Incoming {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(_) => {
            let (tx, rx) = mpsc::channel();
            drop(tx);
            return Incoming { rx, _guard: None };
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    write_info(port);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let stop_t = stop.clone();
    let _ = std::thread::Builder::new()
        .name("rmd-single".into())
        .spawn(move || listen_loop(listener, tx, stop_t));
    Incoming {
        rx,
        _guard: Some(Guard {
            _lock: lock,
            stop,
            port,
        }),
    }
}

fn listen_loop(listener: TcpListener, tx: mpsc::Sender<Vec<PathBuf>>, stop: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(false);
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok((stream, _)) = listener.accept() else {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let paths = read_paths(stream);
        if tx.send(paths).is_err() {
            break;
        }
        wake_ui();
    }
}

fn wake_ui() {
    if let Ok(g) = UI_CTX.lock() {
        if let Some(ctx) = g.as_ref() {
            ctx.request_repaint();
        }
    }
}

pub(crate) fn read_path_lines(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in text.split('\n') {
        let t = line.trim_end_matches('\r').trim();
        if t.is_empty() {
            break;
        }
        if t.starts_with('-') {
            continue;
        }
        paths.push(PathBuf::from(t));
    }
    paths
}

fn read_paths(stream: TcpStream) -> Vec<PathBuf> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut r = BufReader::new(stream);
    let mut text = String::new();
    loop {
        let mut line = String::new();
        match r.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if line.trim_end_matches(['\r', '\n']).is_empty() {
                    break;
                }
                text.push_str(&line);
            }
            Err(_) => break,
        }
    }
    read_path_lines(&text)
}

fn send_open(paths: &[PathBuf]) -> bool {
    let Some(info) = read_info() else {
        return false;
    };
    activate_pid(info.pid);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], info.port));
    for _ in 0..CONNECT_RETRY {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(CONNECT_WAIT_MS)) {
            Ok(mut s) => {
                let _ = s.set_nodelay(true);
                for p in paths {
                    let line = p.to_string_lossy();
                    if line.contains('\n') {
                        continue;
                    }
                    let _ = writeln!(s, "{line}");
                }
                let _ = writeln!(s);
                let _ = s.flush();
                return true;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(CONNECT_WAIT_MS)),
        }
    }
    false
}

fn info_path() -> PathBuf {
    settings::data_dir().join("instance.json")
}

fn lock_path() -> PathBuf {
    settings::data_dir().join("instance.lock")
}

fn write_info(port: u16) {
    let dir = settings::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let info = InstanceInfo {
        pid: std::process::id(),
        port,
    };
    let Ok(json) = serde_json::to_string(&info) else {
        return;
    };
    let path = info_path();
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json.as_bytes()).is_err() {
        return;
    }
    if cfg!(windows) && path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn read_info() -> Option<InstanceInfo> {
    let bytes = std::fs::read(info_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn try_lock() -> Option<File> {
    let dir = settings::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = lock_path();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .open(&path)
            .ok()
    }
    #[cfg(not(windows))]
    {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => Some(f),
            Err(_) => None,
        }
    }
}

fn activate_pid(pid: u32) {
    #[cfg(windows)]
    win::activate(pid);
    #[cfg(not(windows))]
    let _ = pid;
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    type Handle = *mut c_void;
    type Bool = i32;
    type Dword = u32;

    const SW_RESTORE: i32 = 9;
    const GW_OWNER: u32 = 4;

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(cb: unsafe extern "system" fn(Handle, isize) -> Bool, lparam: isize) -> Bool;
        fn GetWindowThreadProcessId(hwnd: Handle, pid: *mut Dword) -> Dword;
        fn IsWindowVisible(hwnd: Handle) -> Bool;
        fn IsIconic(hwnd: Handle) -> Bool;
        fn ShowWindow(hwnd: Handle, cmd: i32) -> Bool;
        fn SetForegroundWindow(hwnd: Handle) -> Bool;
        fn AllowSetForegroundWindow(pid: Dword) -> Bool;
        fn GetWindow(hwnd: Handle, cmd: u32) -> Handle;
    }

    struct Find {
        pid: u32,
        hwnd: Handle,
    }

    unsafe extern "system" fn enum_cb(hwnd: Handle, lparam: isize) -> Bool {
        let st = unsafe { &mut *(lparam as *mut Find) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid != st.pid {
            return 1;
        }
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        if !unsafe { GetWindow(hwnd, GW_OWNER) }.is_null() {
            return 1;
        }
        st.hwnd = hwnd;
        0
    }

    pub fn activate(pid: u32) {
        unsafe {
            let _ = AllowSetForegroundWindow(pid);
            let mut st = Find {
                pid,
                hwnd: std::ptr::null_mut(),
            };
            EnumWindows(enum_cb, &mut st as *mut Find as isize);
            if st.hwnd.is_null() {
                return;
            }
            if IsIconic(st.hwnd) != 0 {
                ShowWindow(st.hwnd, SW_RESTORE);
            }
            SetForegroundWindow(st.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lines_stop_at_blank() {
        let p = read_path_lines("D:\\a.md\r\nD:\\b.md\r\n\r\nignored.md\n");
        assert_eq!(p.len(), 2);
        assert!(p[0].ends_with("a.md"));
        assert!(p[1].ends_with("b.md"));
    }

    #[test]
    fn path_lines_skip_flags() {
        let p = read_path_lines("--selftest\nD:\\c.md\n");
        assert_eq!(p.len(), 1);
        assert!(p[0].ends_with("c.md"));
    }
}
