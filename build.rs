use std::path::PathBuf;
use std::time::Duration;

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=native/pdfium/pdfium.dll");
    kill_running_exe();
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=embed Windows icon failed: {e}");
        }
        copy_pdfium();
    }
}

fn kill_running_exe() {
    if !cfg!(windows) {
        return;
    }
    let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "rustmarkdown".into());
    let image = format!("{pkg}.exe");
    let ok = std::process::Command::new("taskkill")
        .args(["/F", "/IM", &image, "/T"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn copy_pdfium() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("native/pdfium/pdfium.dll");
    if !src.is_file() {
        println!("cargo:warning=missing native/pdfium/pdfium.dll; PDF text selection needs it");
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(profile_dir) = out.ancestors().nth(3) else {
        return;
    };
    let dest = profile_dir.join("pdfium.dll");
    if dest.exists() {
        if let (Ok(a), Ok(b)) = (std::fs::metadata(&src), std::fs::metadata(&dest)) {
            if a.len() == b.len() {
                return;
            }
        }
    }
    if let Err(e) = std::fs::copy(&src, &dest) {
        println!("cargo:warning=copy pdfium.dll failed: {e}");
    }
}
