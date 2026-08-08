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
use std::time::Instant;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// 结束进程后等待其退出的时间，避免新实例撞上旧实例的单例锁
const KILL_SETTLE: Duration = Duration::from_millis(900);

/// 等待进程真正消失的上限。超时说明杀不掉，此时直接报错，
/// 不能继续启动 —— 否则只是把还活着的窗口重新激活，看起来像"没重启"。
#[cfg(target_os = "windows")]
const KILL_TIMEOUT: Duration = Duration::from_secs(8);

#[cfg(target_os = "windows")]
const KILL_POLL: Duration = Duration::from_millis(250);

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
    /// 注意顺序：必须是「应用主进程」优先。
    /// Codex 商店版的主进程其实叫 ChatGPT.exe，`resources\codex.exe` 只是它拉起的
    /// CLI 后端 —— 只杀后端会让应用停在「ChatGPT 意外停止」而不是退出。
    #[cfg(target_os = "windows")]
    fn windows_images(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["Claude.exe", "claude.exe"],
            Self::Codex => &[
                "ChatGPT.exe",
                "Codex.exe",
                "codex-plus-plus.exe",
                "codex-app.exe",
                "codex.exe",
            ],
        }
    }

    /// 判断某个 exe 路径是否属于这个桌面应用。
    /// Claude Code CLI 的 exe 也叫 claude.exe（在 `AppData\Local\Claude-3p\claude-code\`
    /// 下），仅按进程名匹配会把用户正在跑的 CLI 一起杀掉，必须排除。
    #[cfg(target_os = "windows")]
    fn path_belongs_to_app(self, path: &std::path::Path) -> bool {
        let lower = path.to_string_lossy().to_lowercase();
        match self {
            Self::Claude => !lower.contains("claude-code") && !lower.contains("claude-3p"),
            Self::Codex => true,
        }
    }

    /// MSIX（微软商店）包名候选。装在 `C:\Program Files\WindowsApps` 下的应用
    /// ACL 属于 TrustedInstaller，直接 exec 原始 exe 会 os error 5，
    /// 必须走 AUMID 让应用激活管理器拉起，应用才拿得到包标识。
    #[cfg(target_os = "windows")]
    fn msix_package_names(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["Claude", "Anthropic.Claude", "AnthropicClaude"],
            Self::Codex => &["OpenAI.Codex", "OpenAI.CodexApp"],
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
    // 先定位主进程，拿到它所在的安装根目录
    let main_path = app
        .windows_images()
        .iter()
        .find_map(|image| windows_running_path(app, image));
    let install_root = main_path.as_deref().and_then(windows_install_root);
    let was_running = main_path.is_some();

    // 按安装目录（而不是进程名）枚举出该应用的全部进程，一次性杀干净。
    // Electron / Tauri 应用有渲染进程、GPU 进程、辅助服务（如 cowork-svc.exe），
    // 只杀主进程会留下孤儿进程，下次激活就会撞上残留状态。
    if let Some(root) = install_root.as_deref() {
        let pids = windows_pids_under(root, app);
        if !pids.is_empty() {
            windows_kill_pids(&pids);
            // 确认真的退出了再启动，否则 AUMID 激活只会把还活着的窗口拉到前台，
            // 用户看到的就是「没重启」。
            let deadline = Instant::now() + KILL_TIMEOUT;
            loop {
                let alive = windows_pids_under(root, app);
                if alive.is_empty() {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "无法结束 {} 的进程（残留 PID: {}），可能被系统策略保护，请手动退出后重试",
                        app.mac_app_name(),
                        alive
                            .iter()
                            .map(|p| p.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                std::thread::sleep(KILL_POLL);
            }
            // 商店应用退出后系统还要回收包容器，给一点缓冲再激活
            std::thread::sleep(KILL_SETTLE);
        }
    }

    let running_path = main_path;

    // MSIX 优先：AUMID 拉起才拿得到包标识，且不受 WindowsApps 目录 ACL 限制。
    // 非商店安装的应用查不到包，自然回落到 exe 路径。
    if let Some(aumid) = windows_aumid(app) {
        launch_via_aumid(&aumid)?;
        return Ok(RestartDesktopAppResult {
            was_running,
            launched: true,
            path: Some(aumid),
        });
    }

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
        .map_err(|e| {
            format!(
                "启动 {} 失败: {e}（若为商店版应用，请确认包未被系统策略阻止）",
                exe.display()
            )
        })?;

    Ok(RestartDesktopAppResult {
        was_running,
        launched: true,
        path: Some(exe.to_string_lossy().to_string()),
    })
}

/// 查 MSIX 包的 AUMID，形如 `OpenAI.Codex_2p2nqsd0c76g0!App`。
/// 未安装为商店包时返回 None。
#[cfg(target_os = "windows")]
fn windows_aumid(app: DesktopApp) -> Option<String> {
    for name in app.msix_package_names() {
        let script = format!(
            "$p = Get-AppxPackage -Name '{name}' | Select-Object -First 1; \
             if ($p) {{ \
               $id = (Get-AppxPackageManifest $p).Package.Applications.Application \
                     | Select-Object -First 1 -ExpandProperty Id; \
               if ($id) {{ \"$($p.PackageFamilyName)!$id\" }} \
             }}"
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
            .ok();

        if let Some(output) = output {
            let aumid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if aumid.contains('!') {
                return Some(aumid);
            }
        }
    }
    None
}

/// 通过 `shell:AppsFolder` 激活 MSIX 应用。explorer 只负责转交激活请求，
/// 立即返回，因此拿不到目标进程的退出码 —— 用轮询确认是否真的起来了。
#[cfg(target_os = "windows")]
fn launch_via_aumid(aumid: &str) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{aumid}"))
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("激活 {aumid} 失败: {e}"))?;
    Ok(())
}

/// 推断安装根目录。MSIX 应用的进程散落在 `<包目录>\app\` 和
/// `<包目录>\app\resources\` 下，取到包目录才能把它们全部覆盖。
#[cfg(target_os = "windows")]
fn windows_install_root(exe: &std::path::Path) -> Option<PathBuf> {
    let lower = exe.to_string_lossy().to_lowercase();
    if let Some(idx) = lower.find("\\windowsapps\\") {
        // 截到 WindowsApps 下的第一层包目录
        let after = idx + "\\windowsapps\\".len();
        let rest = &lower[after..];
        let pkg_len = rest.find('\\').unwrap_or(rest.len());
        let full = exe.to_string_lossy();
        return Some(PathBuf::from(&full[..after + pkg_len]));
    }
    exe.parent().map(|p| p.to_path_buf())
}

/// 枚举安装目录下所有属于该应用的进程 PID。
#[cfg(target_os = "windows")]
fn windows_pids_under(root: &std::path::Path, app: DesktopApp) -> Vec<u32> {
    let root_str = root.to_string_lossy().replace('\'', "''");
    let script = format!(
        "Get-Process -ErrorAction SilentlyContinue | Where-Object {{ \
           $_.Path -and $_.Path.StartsWith('{root_str}', \
           [System.StringComparison]::OrdinalIgnoreCase) }} | \
         ForEach-Object {{ \"$($_.Id)|$($_.Path)\" }}"
    );
    let Some(output) = Command::new("powershell")
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
        .ok()
    else {
        return Vec::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, path) = line.trim().split_once('|')?;
            let path = PathBuf::from(path);
            app.path_belongs_to_app(&path)
                .then(|| pid.parse::<u32>().ok())
                .flatten()
        })
        .collect()
}

/// 按 PID 结束进程（带子进程树）。用 PID 而非进程名，避免误杀同名的其它程序。
#[cfg(target_os = "windows")]
fn windows_kill_pids(pids: &[u32]) {
    let mut args: Vec<String> = Vec::with_capacity(pids.len() * 2 + 2);
    for pid in pids {
        args.push("/PID".to_string());
        args.push(pid.to_string());
    }
    args.push("/T".to_string());
    args.push("/F".to_string());
    let _ = Command::new("taskkill")
        .args(&args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// 用 PowerShell 取运行中进程的可执行路径。用它而不是写死安装目录，
/// 是因为 Claude Desktop 会把真正的 exe 放进带版本号的 `app-x.y.z` 子目录。
#[cfg(target_os = "windows")]
fn windows_running_path(app: DesktopApp, image: &str) -> Option<PathBuf> {
    let name = image.trim_end_matches(".exe");
    let script = format!(
        "Get-Process -Name '{name}' -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.Path }} | ForEach-Object {{ $_.Path }}"
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

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| PathBuf::from(line.trim()))
        .find(|p| !p.as_os_str().is_empty() && app.path_belongs_to_app(p) && p.is_file())
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
