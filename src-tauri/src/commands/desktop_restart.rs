//! 重启外部桌面应用（Claude Desktop / Codex 桌面版）
//!
//! 桌面端一般只在启动时读取一次配置，切换供应商后需要重启才生效。
//! 策略：先拿到正在运行进程的可执行路径 → 结束进程 → 用同一路径拉起。
//! 拿不到路径时回落到各平台的常见安装位置。

use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// 结束进程后等待其退出的时间，避免新实例撞上旧实例的单例锁
const KILL_SETTLE: Duration = Duration::from_millis(900);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopApp {
    Claude,
    Codex,
}

impl DesktopApp {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-desktop" | "claude_desktop" => Ok(Self::Claude),
            "codex" | "codex-desktop" | "codex_desktop" => Ok(Self::Codex),
            other => Err(format!("未知的桌面应用: {other}")),
        }
    }

    /// macOS 应用名，同时用于 `osascript quit` 与 `open -a`
    fn mac_app_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// Windows 进程名候选，按优先级排列。
    /// 官方包与第三方 GUI（如 Codex++）的 exe 名不同，逐个探测才不会漏。
    #[cfg(target_os = "windows")]
    fn windows_images(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude.exe", "Claude.exe"],
            Self::Codex => &["Codex.exe", "codex-plus-plus.exe", "codex-app.exe"],
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartDesktopAppResult {
    /// 重启前该应用是否在运行
    pub was_running: bool,
    /// 是否成功拉起新实例
    pub launched: bool,
    /// 实际使用的可执行文件 / 应用路径（便于用户排查）
    pub path: Option<String>,
}

/// 重启桌面应用。`target` 取 `claude` 或 `codex`。
#[tauri::command]
pub async fn restart_desktop_app(target: String) -> Result<RestartDesktopAppResult, String> {
    let app = DesktopApp::parse(&target)?;
    tauri::async_runtime::spawn_blocking(move || restart(app))
        .await
        .map_err(|e| format!("重启任务执行失败: {e}"))?
}

// ---------------------------------------------------------------- Windows

#[cfg(target_os = "windows")]
fn restart(app: DesktopApp) -> Result<RestartDesktopAppResult, String> {
    // 逐个候选进程名找出正在跑的那个，顺带拿到它的真实 exe 路径
    let running = app
        .windows_images()
        .iter()
        .find_map(|image| windows_running_path(image).map(|path| (*image, path)));
    let was_running = running.is_some();

    let running_path = match running {
        Some((image, path)) => {
            let _ = Command::new("taskkill")
                .args(["/IM", image, "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            std::thread::sleep(KILL_SETTLE);
            Some(path)
        }
        None => None,
    };

    let exe = running_path
        .or_else(|| {
            windows_fallback_paths(app)
                .into_iter()
                .find(|p| p.is_file())
        })
        .ok_or_else(|| {
            format!(
                "未找到 {} 的可执行文件，请确认已安装桌面版",
                app.mac_app_name()
            )
        })?;

    Command::new(&exe)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {e}", exe.display()))?;

    Ok(RestartDesktopAppResult {
        was_running,
        launched: true,
        path: Some(exe.to_string_lossy().to_string()),
    })
}

/// 用 PowerShell 取运行中进程的可执行路径。用它而不是写死安装目录，
/// 是因为 Claude Desktop 会把真正的 exe 放进带版本号的 `app-x.y.z` 子目录。
#[cfg(target_os = "windows")]
fn windows_running_path(image: &str) -> Option<PathBuf> {
    let name = image.trim_end_matches(".exe");
    let script = format!(
        "(Get-Process -Name '{name}' -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.Path }} | Select-Object -First 1).Path"
    );
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

#[cfg(target_os = "windows")]
fn windows_fallback_paths(app: DesktopApp) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        roots.push(local.join("Programs"));
        roots.push(local);
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var).map(PathBuf::from) {
            roots.push(dir);
        }
    }

    // (安装目录名, exe 名)
    let combos: &[(&str, &str)] = match app {
        DesktopApp::Claude => &[
            ("AnthropicClaude", "claude.exe"),
            ("Claude", "Claude.exe"),
            ("Claude", "claude.exe"),
        ],
        DesktopApp::Codex => &[
            ("Codex", "Codex.exe"),
            ("Codex++", "codex-plus-plus.exe"),
            ("Codex", "codex-app.exe"),
            ("OpenAI Codex", "Codex.exe"),
        ],
    };

    let mut out = Vec::new();
    for root in &roots {
        for (dir, exe) in combos {
            let base = root.join(dir);
            out.push(base.join(exe));
            // Electron/Squirrel 打包会把真 exe 放进带版本号的 app-x.y.z 子目录，
            // 取版本号最大的那个（目录名字典序倒排足够用）。
            if let Ok(entries) = std::fs::read_dir(&base) {
                let mut versioned: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_dir()
                            && p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| n.starts_with("app-"))
                                .unwrap_or(false)
                    })
                    .collect();
                versioned.sort();
                out.extend(versioned.into_iter().rev().map(|p| p.join(exe)));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ macOS

/// 先用 AppleScript 请求退出（给应用保存状态的机会），必要时再 SIGKILL 兜底，
/// 最后 `open -a` 拉起。
#[cfg(target_os = "macos")]
fn restart(app: DesktopApp) -> Result<RestartDesktopAppResult, String> {
    let name = app.mac_app_name();
    let bundle = format!("/Applications/{name}.app");

    let was_running = Command::new("pgrep")
        .args(["-x", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if was_running {
        let _ = Command::new("osascript")
            .args(["-e", &format!("tell application \"{name}\" to quit")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(KILL_SETTLE);
        let _ = Command::new("pkill")
            .args(["-x", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(300));
    }

    let status = Command::new("open")
        .args(["-a", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("启动 {name} 失败: {e}"))?;

    if !status.success() {
        return Err(format!("未找到 {name}.app，请确认已安装桌面版"));
    }

    Ok(RestartDesktopAppResult {
        was_running,
        launched: true,
        path: PathBuf::from(&bundle)
            .exists()
            .then_some(bundle)
            .or(Some(name.to_string())),
    })
}

// ------------------------------------------------------------------ Linux

#[cfg(all(unix, not(target_os = "macos")))]
fn restart(app: DesktopApp) -> Result<RestartDesktopAppResult, String> {
    let candidates: &[&str] = match app {
        DesktopApp::Claude => &["claude-desktop", "claude"],
        DesktopApp::Codex => &["codex-app", "codex-desktop"],
    };

    let exe = candidates
        .iter()
        .find_map(|name| {
            let out = Command::new("which").arg(name).output().ok()?;
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!path.is_empty()).then(|| PathBuf::from(path))
        })
        .ok_or_else(|| {
            format!(
                "未找到 {} 的桌面版可执行文件（尝试过: {}）",
                app.mac_app_name(),
                candidates.join(", ")
            )
        })?;

    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let was_running = Command::new("pgrep")
        .args(["-x", &name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if was_running {
        let _ = Command::new("pkill")
            .args(["-x", &name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        std::thread::sleep(KILL_SETTLE);
    }

    Command::new(&exe)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {e}", exe.display()))?;

    Ok(RestartDesktopAppResult {
        was_running,
        launched: true,
        path: Some(exe.to_string_lossy().to_string()),
    })
}
