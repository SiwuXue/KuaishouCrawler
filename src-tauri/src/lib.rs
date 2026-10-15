use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default, Clone)]
struct CrawlerManager {
    current: Arc<Mutex<CurrentProcess>>,
}

#[derive(Default)]
struct CurrentProcess {
    child: Option<Arc<Mutex<Child>>>,
    pid: Option<u32>,
    kind: String,
    status: String,
    target: String,
    started_at: Option<u64>,
    stop_requested: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskStatus {
    status: String,
    kind: String,
    target: String,
    started_at: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineStatus {
    ready: bool,
    python_installed: bool,
    browser_installed: bool,
    engine_installed: bool,
    message: String,
}

#[derive(Clone, Serialize)]
struct AppLog {
    id: u128,
    timestamp: String,
    level: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CrawlRequest {
    mode: String,
    target: String,
    start_page: u32,
    max_items: u32,
    max_comments: u32,
    include_comments: bool,
    include_replies: bool,
    save_format: String,
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn emit_log(app: &AppHandle, level: &str, message: impl Into<String>) {
    let log = AppLog {
        id: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        timestamp: now_epoch().to_string(),
        level: level.to_string(),
        message: message.into(),
    };
    let _ = app.emit("crawler-log", log);
}

fn emit_status(app: &AppHandle, manager: &CrawlerManager) {
    if let Ok(current) = manager.current.lock() {
        let status = TaskStatus {
            status: if current.status.is_empty() {
                "idle".to_string()
            } else {
                current.status.clone()
            },
            kind: current.kind.clone(),
            target: current.target.clone(),
            started_at: current.started_at,
        };
        let _ = app.emit("crawler-status", status);
    }
}

fn app_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(resources) = app.path().resource_dir() {
        if resources.join("backend").join("desktop_helper.py").is_file() {
            return Ok(resources);
        }
    }

    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    if manifest_root.join("backend").join("desktop_helper.py").is_file() {
        return manifest_root
            .canonicalize()
            .map_err(|error| format!("无法定位项目目录：{error}"));
    }

    Err("没有找到随应用提供的本地采集引擎文件。".to_string())
}

fn backend_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_root(app)?.join("backend"))
}

fn engine_root(app: &AppHandle) -> Result<PathBuf, String> {
    let path = backend_root(app)?.join("engine");
    if path.join("main.py").is_file() {
        Ok(path)
    } else {
        Err("采集引擎文件不完整，请重新安装本机运行环境。".to_string())
    }
}

fn runtime_roots(app: &AppHandle) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(root) = backend_root(app) {
        roots.push(root.join(".runtime"));
    }
    if let Ok(root) = app.path().app_local_data_dir() {
        let installed = root.join(".runtime");
        if !roots.iter().any(|candidate| candidate == &installed) {
            roots.push(installed);
        }
    }
    roots
}

fn python_path(runtime_root: &Path) -> Option<PathBuf> {
    let marker = runtime_root.join("python-path.txt");
    let relative = std::fs::read_to_string(marker).ok()?;
    let relative = relative.trim().trim_start_matches('\u{feff}');
    let path = runtime_root.join(relative);
    path.is_file().then_some(path)
}

fn runtime_status(app: &AppHandle) -> EngineStatus {
    let engine_installed = engine_root(app).is_ok();
    let roots = runtime_roots(app);
    let python = roots.iter().find_map(|root| python_path(root));
    let browser_installed = roots
        .iter()
        .any(|root| root.join("browser-installed.flag").is_file());
    let python_installed = python.is_some();
    let ready = engine_installed && python_installed && browser_installed;
    let message = if !engine_installed {
        "采集引擎文件不完整".to_string()
    } else if !python_installed {
        "尚未安装项目专用 Python 运行环境".to_string()
    } else if !browser_installed {
        "尚未安装项目专用 Chromium 浏览器".to_string()
    } else {
        "采集引擎与浏览器已就绪".to_string()
    };

    EngineStatus {
        ready,
        python_installed,
        browser_installed,
        engine_installed,
        message,
    }
}

fn resolve_python(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    for root in runtime_roots(app) {
        if let Some(path) = python_path(&root) {
            return Ok((path, root));
        }
    }
    Err("本机运行环境尚未安装。请到“设置”页安装采集引擎和浏览器。".to_string())
}

fn resolve_output_root(app: &AppHandle, configured: &str) -> Result<PathBuf, String> {
    let configured = configured.trim();
    if configured.is_empty() {
        return Err("请先在设置中填写输出目录。".to_string());
    }
    let configured_path = PathBuf::from(configured);
    let resolved = if configured_path.is_absolute() {
        configured_path
    } else {
        app.path()
            .app_local_data_dir()
            .map_err(|error| format!("无法读取应用数据目录：{error}"))?
            .join(configured_path)
    };
    std::fs::create_dir_all(&resolved)
        .map_err(|error| format!("无法创建输出目录：{error}"))?;
    resolved
        .canonicalize()
        .map_err(|error| format!("无法访问输出目录：{error}"))
}

fn helper_command(app: &AppHandle, args: &[String]) -> Result<String, String> {
    let (python, runtime_root) = resolve_python(app)?;
    let backend = backend_root(app)?;
    let helper = backend.join("desktop_helper.py");
    let engine = engine_root(app)?;
    let mut command = Command::new(python);
    command
        .arg(helper)
        .args(args)
        .current_dir(engine)
        .env(
            "PLAYWRIGHT_BROWSERS_PATH",
            runtime_root.join("ms-playwright"),
        )
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("无法读取本地文件：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        Ok(stdout)
    } else {
        if let Ok(value) = serde_json::from_str::<Value>(&stdout) {
            if let Some(error) = value.get("error").and_then(Value::as_str) {
                return Err(error.to_string());
            }
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "读取本地文件失败。".to_string()
        } else {
            stderr
        })
    }
}

fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
}

fn add_project_tools_to_path(command: &mut Command, backend: &Path) {
    let mut entries = vec![backend.join("tools")];
    if let Some(existing) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&existing));
    }
    if let Ok(path) = std::env::join_paths(entries) {
        command.env("PATH", path);
    }
}

fn log_level(line: &str) -> &'static str {
    let upper = line.to_ascii_uppercase();
    if upper.contains("ERROR") || upper.contains("FAILED") || line.contains("失败") {
        "error"
    } else if upper.contains("WARNING") || upper.contains("WARN") || line.contains("警告") {
        "warning"
    } else if upper.contains("SUCCESS") || line.contains("成功") || line.contains("完成") {
        "success"
    } else if upper.contains("DEBUG") {
        "debug"
    } else {
        "info"
    }
}

fn pump_output(app: AppHandle, stream: impl std::io::Read + Send + 'static) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            match line {
                Ok(message) if !message.trim().is_empty() => {
                    let level = log_level(&message);
                    emit_log(&app, level, message);
                }
                Ok(_) => {}
                Err(error) => {
                    emit_log(&app, "warning", format!("读取任务输出时遇到问题：{error}"));
                    break;
                }
            }
        }
    });
}

fn spawn_managed(
    app: &AppHandle,
    manager: &CrawlerManager,
    mut command: Command,
    kind: &str,
    target: &str,
) -> Result<(), String> {
    let mut current = manager
        .current
        .lock()
        .map_err(|_| "任务状态暂时不可用。".to_string())?;
    if current.child.is_some() {
        return Err("已有任务正在运行，请先等待完成或停止当前任务。".to_string());
    }

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动本地任务失败：{error}"))?;
    let pid = child.id();
    if let Some(stdout) = child.stdout.take() {
        pump_output(app.clone(), stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        pump_output(app.clone(), stderr);
    }
    let child = Arc::new(Mutex::new(child));

    current.child = Some(child.clone());
    current.pid = Some(pid);
    current.kind = kind.to_string();
    current.status = "running".to_string();
    current.target = target.to_string();
    current.started_at = Some(now_epoch());
    current.stop_requested = false;
    drop(current);

    emit_status(app, manager);
    let app_for_monitor = app.clone();
    let manager_for_monitor = manager.clone();
    let kind = kind.to_string();
    thread::spawn(move || {
        let exit_status = loop {
            let status = child
                .lock()
                .ok()
                .and_then(|mut process| process.try_wait().ok().flatten());
            if let Some(status) = status {
                break Some(status);
            }
            if child.lock().is_err() {
                break None;
            }
            thread::sleep(Duration::from_millis(300));
        };

        let (stopped, should_emit) = if let Ok(mut current) = manager_for_monitor.current.lock() {
            let matches_child = current.pid == Some(pid);
            if matches_child {
                let stopped = current.stop_requested;
                current.child = None;
                current.pid = None;
                current.status = if exit_status.as_ref().is_some_and(|status| status.success()) {
                    "idle".to_string()
                } else if stopped {
                    "idle".to_string()
                } else {
                    "error".to_string()
                };
                current.stop_requested = false;
                (stopped, true)
            } else {
                (false, false)
            }
        } else {
            (false, false)
        };

        if should_emit {
            if stopped {
                emit_log(&app_for_monitor, "warning", "当前任务已停止".to_string());
            } else if exit_status.as_ref().is_some_and(|status| status.success()) {
                emit_log(&app_for_monitor, "success", "采集任务已完成".to_string());
            } else {
                let code = exit_status
                    .and_then(|status| status.code())
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "未知".to_string());
                emit_log(
                    &app_for_monitor,
                    "error",
                    format!("任务异常退出（代码 {code}），请查看上方日志。"),
                );
            }
            emit_status(&app_for_monitor, &manager_for_monitor);
            if kind == "setup" {
                let _ = app_for_monitor.emit("engine-status", runtime_status(&app_for_monitor));
            }
        }
    });
    Ok(())
}

#[tauri::command]
fn get_task_status(manager: State<'_, CrawlerManager>) -> TaskStatus {
    let current = manager.current.lock().expect("task status lock poisoned");
    TaskStatus {
        status: if current.status.is_empty() {
            "idle".to_string()
        } else {
            current.status.clone()
        },
        kind: current.kind.clone(),
        target: current.target.clone(),
        started_at: current.started_at,
    }
}

#[tauri::command]
fn get_engine_status(app: AppHandle) -> EngineStatus {
    runtime_status(&app)
}

#[tauri::command]
fn setup_backend(
    app: AppHandle,
    manager: State<'_, CrawlerManager>,
) -> Result<(), String> {
    let root = app_root(&app)?;
    let backend = root.join("backend");
    let engine = engine_root(&app)?;
    let script = root.join("scripts").join("setup-backend.ps1");
    if !script.is_file() {
        return Err("没有找到环境安装脚本。".to_string());
    }

    #[cfg(debug_assertions)]
    let runtime = backend.join(".runtime");
    #[cfg(not(debug_assertions))]
    let runtime = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法创建应用运行目录：{error}"))?
        .join(".runtime");

    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg("-EngineRoot")
        .arg(engine)
        .arg("-RuntimeRoot")
        .arg(runtime)
        .current_dir(root);
    spawn_managed(&app, &manager, command, "setup", "安装本机运行环境")
}

#[tauri::command]
fn start_crawler(
    app: AppHandle,
    manager: State<'_, CrawlerManager>,
    request: CrawlRequest,
    output_directory: String,
) -> Result<(), String> {
    let (python, runtime_root) = resolve_python(&app)?;
    let backend = backend_root(&app)?;
    let engine = engine_root(&app)?;
    let output_root = resolve_output_root(&app, &output_directory)?;
    let browser_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("无法读取应用数据目录：{error}"))?
        .join("browser-data")
        .join("ks_user_data_dir");
    let mode = request.mode.as_str();
    if !["search", "detail", "creator"].contains(&mode) {
        return Err("请选择有效的采集方式。".to_string());
    }
    let valid_formats = ["jsonl", "json", "csv", "excel"];
    if !valid_formats.contains(&request.save_format.as_str()) {
        return Err("请选择有效的保存格式。".to_string());
    }
    if request.target.trim().is_empty() {
        return Err("请填写采集目标。".to_string());
    }
    if request.max_items == 0 || request.max_items > 10_000 {
        return Err("作品数量请设置为 1 到 10000 之间。".to_string());
    }

    let crawler_type = match mode {
        "search" => "关键词搜索",
        "detail" => "视频详情",
        _ => "创作者主页",
    };
    let target = request.target.trim().to_string();
    let mut command = Command::new(python);
    command
        .arg("main.py")
        .args(["--platform", "ks", "--lt", "qrcode", "--type", mode])
        .args(["--save_data_option", request.save_format.as_str()])
        .args(["--save_data_path"])
        .arg(&output_root)
        .args(["--start", &request.start_page.max(1).to_string()])
        .args(["--crawler_max_notes_count", &request.max_items.to_string()])
        .args(["--get_comment", if request.include_comments { "true" } else { "false" }])
        .args(["--get_sub_comment", if request.include_replies { "true" } else { "false" }])
        .args(["--max_comments_count_singlenotes", &request.max_comments.max(1).to_string()])
        .current_dir(&engine)
        .env("PLAYWRIGHT_BROWSERS_PATH", runtime_root.join("ms-playwright"))
        .env("KUAISHOU_CRAWLER_USER_DATA_DIR", browser_data)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");
    add_project_tools_to_path(&mut command, &backend);

    match mode {
        "search" => {
            command.args(["--keywords", target.as_str()]);
        }
        "detail" => {
            let ids = target
                .lines()
                .flat_map(|line| line.split(','))
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            command.args(["--specified_id", ids.as_str()]);
        }
        "creator" => {
            let ids = target
                .lines()
                .flat_map(|line| line.split(','))
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            command.args(["--creator_id", ids.as_str()]);
        }
        _ => unreachable!(),
    }

    emit_log(
        &app,
        "info",
        format!("已提交{}任务，输出目录：{}", crawler_type, output_root.display()),
    );
    spawn_managed(&app, &manager, command, "crawler", &target)
}

#[tauri::command]
fn stop_crawler(
    app: AppHandle,
    manager: State<'_, CrawlerManager>,
) -> Result<(), String> {
    let mut current = manager
        .current
        .lock()
        .map_err(|_| "任务状态暂时不可用。".to_string())?;
    let _child = current
        .child
        .clone()
        .ok_or_else(|| "当前没有正在运行的采集任务。".to_string())?;
    if current.kind != "crawler" {
        return Err("环境安装期间暂不能启动或停止采集任务。".to_string());
    }
    current.status = "stopping".to_string();
    current.stop_requested = true;
    let pid = current.pid.ok_or_else(|| "无法读取任务进程。".to_string())?;
    drop(current);

    emit_log(&app, "warning", "正在停止采集任务…");
    emit_status(&app, &manager);
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill.exe");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        hide_console(&mut command);
        let _ = command.status();
    }
    #[cfg(not(windows))]
    {
        if let Ok(mut process) = _child.lock() {
            let _ = process.kill();
        }
    }
    Ok(())
}

#[tauri::command]
fn list_data_files(app: AppHandle, output_directory: String) -> Result<Value, String> {
    let root = resolve_output_root(&app, &output_directory)?;
    let args = vec!["list-files".to_string(), root.to_string_lossy().into_owned()];
    let output = helper_command(&app, &args)?;
    serde_json::from_str(&output).map_err(|error| format!("无法解析本地文件列表：{error}"))
}

#[tauri::command]
fn preview_data_file(
    app: AppHandle,
    output_directory: String,
    relative_path: String,
) -> Result<Value, String> {
    let root = resolve_output_root(&app, &output_directory)?;
    let args = vec![
        "preview".to_string(),
        root.to_string_lossy().into_owned(),
        relative_path,
    ];
    let output = helper_command(&app, &args)?;
    serde_json::from_str(&output).map_err(|error| format!("无法解析文件预览：{error}"))
}

#[tauri::command]
fn export_data_file(
    app: AppHandle,
    output_directory: String,
    relative_path: String,
    destination: String,
) -> Result<(), String> {
    let root = resolve_output_root(&app, &output_directory)?;
    let source = root.join(relative_path);
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("无法访问输出目录：{error}"))?;
    let canonical_source = source
        .canonicalize()
        .map_err(|error| format!("无法访问采集文件：{error}"))?;
    if !canonical_source.starts_with(&canonical_root) || !canonical_source.is_file() {
        return Err("文件路径无效。".to_string());
    }
    let destination = PathBuf::from(destination);
    if !destination.is_absolute() {
        return Err("请选择有效的导出位置。".to_string());
    }
    std::fs::copy(canonical_source, &destination)
        .map(|_| ())
        .map_err(|error| format!("导出文件失败：{error}"))
}

fn terminate_active(manager: &CrawlerManager) {
    let Ok(mut current) = manager.current.lock() else {
        return;
    };
    let Some(_child) = current.child.clone() else {
        return;
    };
    current.stop_requested = true;
    let pid = current.pid;
    drop(current);
    #[cfg(windows)]
    if let Some(pid) = pid {
        let mut command = Command::new("taskkill.exe");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        hide_console(&mut command);
        let _ = command.status();
    }
    #[cfg(not(windows))]
    if let Ok(mut process) = _child.lock() {
        let _ = process.kill();
    }
}

pub fn run() {
    let manager = CrawlerManager::default();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(manager)
        .invoke_handler(tauri::generate_handler![
            get_task_status,
            get_engine_status,
            setup_backend,
            start_crawler,
            stop_crawler,
            list_data_files,
            preview_data_file,
            export_data_file
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                terminate_active(&window.state::<CrawlerManager>());
            }
        })
        .run(tauri::generate_context!())
        .expect("启动快手采集工作台失败");
}
