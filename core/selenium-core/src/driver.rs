//! Locates, downloads and spawns chromedriver.

use crate::error::{Error, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

pub struct DriverProcess {
    child: std::sync::Mutex<Child>,
    pub port: u16,
}

impl DriverProcess {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl DriverProcess {
    /// Terminates the driver (and with it the browser); safe to call repeatedly.
    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().start_kill();
    }
}

impl Drop for DriverProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

pub async fn spawn(
    driver_path: Option<&Path>,
    chrome_binary: Option<&Path>,
) -> Result<DriverProcess> {
    let exe = resolve(driver_path, chrome_binary).await?;
    let port = free_port()?;
    let child = Command::new(&exe)
        .arg(format!("--port={port}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::launch(format!("cannot start {}: {e}", exe.display())))?;
    let proc = DriverProcess {
        child: std::sync::Mutex::new(child),
        port,
    };

    let client = reqwest::Client::new();
    for _ in 0..100 {
        if client
            .get(format!("{}/status", proc.base_url()))
            .send()
            .await
            .is_ok()
        {
            return Ok(proc);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(Error::launch("chromedriver did not become ready"))
}

fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| Error::launch(e.to_string()))?;
    Ok(l.local_addr()
        .map_err(|e| Error::launch(e.to_string()))?
        .port())
}

async fn resolve(explicit: Option<&Path>, chrome_binary: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Some(p) = find_on_path("chromedriver") {
        return Ok(p);
    }
    download(chrome_binary).await
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(&exe))
        .find(|p| p.is_file())
}

fn chrome_executable(custom: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = custom {
        return Ok(p.to_path_buf());
    }
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]
    } else if cfg!(windows) {
        &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ]
    } else {
        &[
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
        ]
    };
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .ok_or_else(|| Error::launch("Chrome not found; set binaryPath"))
}

fn platform() -> Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-arm64",
        ("macos", "x86_64") => "mac-x64",
        ("linux", "x86_64") => "linux64",
        ("windows", "x86_64") => "win64",
        (os, arch) => return Err(Error::launch(format!("unsupported platform {os}/{arch}"))),
    })
}

async fn chrome_build(custom: Option<&Path>) -> Result<String> {
    let exe = chrome_executable(custom)?;
    let out = if cfg!(windows) {
        // `--version` is unreliable on Windows; read the version from the install dir name.
        let dir = exe
            .parent()
            .ok_or_else(|| Error::launch("bad chrome path"))?;
        std::fs::read_dir(dir)
            .ok()
            .and_then(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .find(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit()))
            })
            .ok_or_else(|| Error::launch("cannot detect Chrome version"))?
    } else {
        let o = Command::new(&exe)
            .arg("--version")
            .output()
            .await
            .map_err(|e| Error::launch(format!("cannot run Chrome: {e}")))?;
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let ver = out
        .split_whitespace()
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .ok_or_else(|| Error::launch("cannot parse Chrome version"))?;
    Ok(ver.split('.').take(3).collect::<Vec<_>>().join("."))
}

async fn download(chrome_binary: Option<&Path>) -> Result<PathBuf> {
    let build = chrome_build(chrome_binary).await?;
    let platform = platform()?;
    let cache = dirs::cache_dir()
        .ok_or_else(|| Error::launch("no cache dir"))?
        .join("selenium-next")
        .join("chromedriver")
        .join(&build);
    let exe_name = if cfg!(windows) {
        "chromedriver.exe"
    } else {
        "chromedriver"
    };
    let exe = cache.join(exe_name);
    if exe.is_file() {
        return Ok(exe);
    }

    let http = reqwest::Client::new();
    let index: Value = http
        .get("https://googlechromelabs.github.io/chrome-for-testing/latest-patch-versions-per-build-with-downloads.json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| Error::launch(format!("driver index: {e}")))?
        .json()
        .await
        .map_err(|e| Error::launch(format!("driver index: {e}")))?;
    let url = index["builds"][&build]["downloads"]["chromedriver"]
        .as_array()
        .and_then(|a| a.iter().find(|d| d["platform"] == platform))
        .and_then(|d| d["url"].as_str())
        .ok_or_else(|| {
            Error::launch(format!(
                "no chromedriver for Chrome build {build} ({platform})"
            ))
        })?
        .to_string();

    let bytes = http
        .get(&url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| Error::launch(format!("driver download: {e}")))?
        .bytes()
        .await
        .map_err(|e| Error::launch(format!("driver download: {e}")))?;

    let cache2 = cache.clone();
    tokio::task::spawn_blocking(move || extract(&bytes, &cache2))
        .await
        .map_err(|e| Error::launch(e.to_string()))??;
    Ok(exe)
}

fn extract(bytes: &[u8], dest: &Path) -> Result<()> {
    let io = |e: std::io::Error| Error::launch(format!("extract: {e}"));
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::launch(e.to_string()))?;
    std::fs::create_dir_all(dest).map_err(io)?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| Error::launch(e.to_string()))?;
        let Some(name) = f
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.to_owned()))
        else {
            continue;
        };
        if f.is_dir() || !name.to_string_lossy().starts_with("chromedriver") {
            continue;
        }
        let target = dest.join(name);
        let mut out = std::fs::File::create(&target).map_err(io)?;
        std::io::copy(&mut f, &mut out).map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                .map_err(io)?;
        }
    }
    Ok(())
}
