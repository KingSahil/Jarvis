use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewWindow, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[derive(Debug, Deserialize)]
struct TutorRequest {
    question: String,
    previous_question: Option<String>,
    progress: Option<serde_json::Value>,
}

#[derive(Clone, Serialize)]
struct GlobalClick {
    x: i32,
    y: i32,
    overlay_x: i32,
    overlay_y: i32,
    scale_factor: f64,
}

enum SttMessage {
    Audio(Vec<u8>),
    Close,
}

struct AppState {
    child: std::sync::Mutex<Option<std::process::Child>>,
    child_stdin: std::sync::Mutex<Option<std::process::ChildStdin>>,
    child_stdout: std::sync::Mutex<Option<BufReader<std::process::ChildStdout>>>,
    stt_tx: std::sync::Mutex<Option<std::sync::mpsc::Sender<SttMessage>>>,
}

#[tauri::command]
async fn run_tutor(app: AppHandle, request: TutorRequest) -> Result<serde_json::Value, String> {
    let overlay = app.get_webview_window("overlay");
    let command = app.get_webview_window("command");

    let overlay_was_visible = overlay.as_ref().map(|w| w.is_visible().unwrap_or(false)).unwrap_or(false);

    // Exclude windows from captures programmatically so they are not captured by the system,
    // while remaining fully visible and active for the user.
    if let Some(ref w) = overlay {
        set_window_capture_exclusion(w, true);
    }
    if let Some(ref w) = command {
        set_window_capture_exclusion(w, true);
    }

    // Give DWM a tiny moment to apply display affinity before screenshot
    thread::sleep(Duration::from_millis(40));

    let state = app.state::<AppState>();
    let output_res = run_python_worker(
        &app,
        &state,
        &request.question,
        request.previous_question.as_deref(),
        request.progress.as_ref(),
        command.clone(),
        overlay.clone(),
    );


    // Fallback: restore capture visibility in case of errors or if not restored by worker
    if let Some(ref w) = command {
        set_window_capture_exclusion(w, false);
    }
    if let Some(ref w) = overlay {
        set_window_capture_exclusion(w, false);
    }

    let output = output_res.map_err(|error| error)?;

    let parsed: serde_json::Value = serde_json::from_str(&output)
        .map_err(|err| format!("Python worker returned invalid JSON: {err}. Raw: {output}"))?;

    // Now restore/show overlay with highlights if applicable
    if let Some(ref w) = overlay {
        if overlay_was_visible || parsed.get("steps").and_then(|s| s.as_array()).map(|a| !a.is_empty()).unwrap_or(false) {
            configure_overlay_passthrough(w);
            let _ = w.emit("blinky://guidance", parsed.clone());
            let _ = w.show();
        }
    }

    Ok(parsed)
}

#[tauri::command]
fn show_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(overlay) = app.get_webview_window("overlay") {
        configure_overlay_passthrough(&overlay);
        overlay.show().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn hide_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(overlay) = app.get_webview_window("overlay") {
        overlay.hide().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn show_command_bar(app: AppHandle) -> Result<(), String> {
    show_command_window(&app);
    Ok(())
}

#[tauri::command]
fn resize_command_window(app: AppHandle, height: f64) -> Result<(), String> {
    if let Some(command) = app.get_webview_window("command") {
        let current_size = command.inner_size().unwrap_or(tauri::PhysicalSize::new(760, 580));
        let scale_factor = command.scale_factor().unwrap_or(1.0);
        let current_logical_width = current_size.width as f64 / scale_factor;
        let size = tauri::LogicalSize::new(current_logical_width, height);
        let _ = command.set_size(size);
    }
    Ok(())
}

#[tauri::command]
fn resize_and_move_command_window(app: AppHandle, x: f64, y: f64, width: f64, height: f64) -> Result<(), String> {
    if let Some(command) = app.get_webview_window("command") {
        let size = tauri::LogicalSize::new(width, height);
        let pos = tauri::LogicalPosition::new(x, y);
        let _ = command.set_size(size);
        let _ = command.set_position(pos);
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct BlinkySettings {
    provider: String,
    shortcut: String,
    sarvam_api_key: String,
    groq_api_key: String,
}

#[tauri::command]
async fn get_settings(app: AppHandle) -> Result<BlinkySettings, String> {
    let root = project_root(&app)?;
    let env_vars = read_env_file(&root);
    
    let mut provider = "groq".to_string();
    let mut shortcut = "Enter".to_string();
    let mut sarvam_api_key = "".to_string();
    let mut groq_api_key = "".to_string();
    
    for (key, val) in env_vars {
        if key == "BLINKY_AI_PROVIDER" {
            provider = val.to_lowercase();
        } else if key == "BLINKY_SHORTCUT" {
            shortcut = val;
        } else if key == "SARVAM_API_KEY" {
            sarvam_api_key = val;
        } else if key == "GROQ_API_KEY" {
            groq_api_key = val;
        }
    }
    
    Ok(BlinkySettings { provider, shortcut, sarvam_api_key, groq_api_key })
}

#[tauri::command]
async fn save_settings(app: AppHandle, provider: String, shortcut: String, sarvam_api_key: String, groq_api_key: String) -> Result<(), String> {
    let root = project_root(&app)?;
    ensure_env_file(&root);
    let env_path = root.join(".env");
    
    // Read the current contents of .env
    let contents = std::fs::read_to_string(&env_path).unwrap_or_default();
    
    // Parse lines, update values, and rebuild
    let mut lines: Vec<String> = contents.lines().map(|s| s.to_string()).collect();
    let mut provider_found = false;
    let mut shortcut_found = false;
    let mut sarvam_api_key_found = false;
    let mut groq_api_key_found = false;
    
    for line in lines.iter_mut() {
        let trimmed = line.trim();
        if trimmed.starts_with("BLINKY_AI_PROVIDER=") {
            *line = format!("BLINKY_AI_PROVIDER={}", provider);
            provider_found = true;
        } else if trimmed.starts_with("BLINKY_SHORTCUT=") {
            *line = format!("BLINKY_SHORTCUT={}", shortcut);
            shortcut_found = true;
        } else if trimmed.starts_with("SARVAM_API_KEY=") {
            *line = format!("SARVAM_API_KEY={}", sarvam_api_key);
            sarvam_api_key_found = true;
        } else if trimmed.starts_with("GROQ_API_KEY=") {
            *line = format!("GROQ_API_KEY={}", groq_api_key);
            groq_api_key_found = true;
        }
    }
    
    if !provider_found {
        lines.push(format!("BLINKY_AI_PROVIDER={}", provider));
    }
    if !shortcut_found {
        lines.push(format!("BLINKY_SHORTCUT={}", shortcut));
    }
    if !sarvam_api_key_found {
        lines.push(format!("SARVAM_API_KEY={}", sarvam_api_key));
    }
    if !groq_api_key_found {
        lines.push(format!("GROQ_API_KEY={}", groq_api_key));
    }
    
    let new_contents = lines.join("\n") + "\n";
    std::fs::write(&env_path, new_contents)
        .map_err(|err| format!("Failed to write .env file: {err}"))?;
        
    Ok(())
}

#[tauri::command]
async fn start_stt_stream(app: AppHandle) -> Result<(), String> {
    let root = project_root(&app)?;
    let env_vars = read_env_file(&root);
    let mut api_key = "".to_string();
    for (key, val) in env_vars {
        if key == "SARVAM_API_KEY" {
            api_key = val;
        }
    }

    if api_key.is_empty() {
        return Err("SARVAM_API_KEY is not set in settings".to_string());
    }

    let (tx, rx) = std::sync::mpsc::channel::<SttMessage>();
    
    let state = app.state::<AppState>();
    let mut stt_tx_guard = state.stt_tx.lock().map_err(|err| err.to_string())?;
    *stt_tx_guard = Some(tx);

    let app_clone = app.clone();
    thread::spawn(move || {
        let url = "wss://api.sarvam.ai/speech-to-text/ws";
        
        let request = http::Request::builder()
            .uri(url)
            .header("api-subscription-key", &api_key)
            .body(())
            .expect("Failed to build request");

        let mut socket = match tungstenite::connect(request) {
            Ok((socket, _)) => socket,
            Err(err) => {
                eprintln!("Failed to connect to Sarvam STT WebSocket: {:?}", err);
                return;
            }
        };

        let stream = socket.get_mut();
        match stream {
            tungstenite::stream::MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(Some(Duration::from_millis(50)));
            }
            tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                let _ = s.get_mut().set_read_timeout(Some(Duration::from_millis(50)));
            }
            _ => {}
        }


        let config = serde_json::json!({
            "type": "config",
            "data": {
                "model": "saaras:v3",
                "language_code": "en-IN",
                "audio_format": "pcm_s16le"
            }
        });
        if let Err(err) = socket.send(tungstenite::Message::Text(config.to_string())) {
            eprintln!("Failed to send config message: {:?}", err);
            return;
        }

        let mut last_ping = std::time::Instant::now();

        loop {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SttMessage::Audio(data) => {
                        if let Err(err) = socket.send(tungstenite::Message::Binary(data)) {
                            eprintln!("Failed to send audio chunk: {:?}", err);
                            break;
                        }
                    }
                    SttMessage::Close => {
                        let _ = socket.close(None);
                        return;
                    }
                }
            }

            if last_ping.elapsed() >= Duration::from_secs(20) {
                let ping = serde_json::json!({
                    "type": "ping"
                });
                if let Err(err) = socket.send(tungstenite::Message::Text(ping.to_string())) {
                    eprintln!("Failed to send keepalive ping: {:?}", err);
                    break;
                }
                last_ping = std::time::Instant::now();
            }

            match socket.read() {
                Ok(tungstenite::Message::Text(text)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(transcript) = json.get("transcript").and_then(|t| t.as_str()) {
                            let _ = app_clone.emit("blinky://stt-transcript", transcript);
                        } else if let Some(model_output) = json.get("model_output").and_then(|m| m.as_str()) {
                            let _ = app_clone.emit("blinky://stt-transcript", model_output);
                        } else if let Some(t) = json.get("text").and_then(|t| t.as_str()) {
                            let _ = app_clone.emit("blinky://stt-transcript", t);
                        }
                    }
                }
                Ok(tungstenite::Message::Close(_)) => {
                    break;
                }
                Err(tungstenite::Error::Io(ref err)) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                    // Expected timeout
                }
                Err(err) => {
                    eprintln!("WebSocket read error: {:?}", err);
                    break;
                }
                _ => {}
            }

            thread::sleep(Duration::from_millis(10));
        }
    });

    Ok(())
}

#[tauri::command]
async fn send_audio_chunk(app: AppHandle, chunk: Vec<u8>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let stt_tx_guard = state.stt_tx.lock().map_err(|err| err.to_string())?;
    if let Some(ref tx) = *stt_tx_guard {
        let _ = tx.send(SttMessage::Audio(chunk));
    }
    Ok(())
}

#[tauri::command]
async fn stop_stt_stream(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut stt_tx_guard = state.stt_tx.lock().map_err(|err| err.to_string())?;
    if let Some(tx) = stt_tx_guard.take() {
        let _ = tx.send(SttMessage::Close);
    }
    Ok(())
}

fn get_active_shortcut_from_env(app: &AppHandle) -> String {
    if let Ok(root) = project_root(app) {
        let env_vars = read_env_file(&root);
        for (key, val) in env_vars {
            if key == "BLINKY_SHORTCUT" {
                return val;
            }
        }
    }
    "Enter".to_string()
}

fn get_or_spawn_worker(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let mut child_guard = state.child.lock().map_err(|err| err.to_string())?;

    let is_running = if let Some(ref mut child) = *child_guard {
        match child.try_wait() {
            Ok(None) => true, // Still running
            _ => false,       // Exited or error
        }
    } else {
        false
    };

    if !is_running {
        let root = project_root(app)?;
        let script = root.join("python").join("main.py");
        let python = python_executable(&root);
        let env_file_vars = read_env_file(&root);

        let mut child = Command::new(python)
            .arg(script)
            .current_dir(&root)
            .env("PYTHONWARNINGS", "ignore")
            .envs(env_file_vars)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("Failed to start persistent Python worker: {err}"))?;

        let child_stdin = child.stdin.take().ok_or("Failed to open child stdin")?;
        let child_stdout = child.stdout.take().ok_or("Failed to open child stdout")?;
        let reader = BufReader::new(child_stdout);

        *child_guard = Some(child);
        *state.child_stdin.lock().map_err(|err| err.to_string())? = Some(child_stdin);
        *state.child_stdout.lock().map_err(|err| err.to_string())? = Some(reader);
        println!("Persistent Python worker spawned successfully!");
    }

    Ok(())
}

fn run_python_worker(
    app: &AppHandle,
    state: &AppState,
    question: &str,
    previous_question: Option<&str>,
    progress: Option<&serde_json::Value>,
    command_window: Option<WebviewWindow>,
    overlay_window: Option<WebviewWindow>,
) -> Result<String, String> {
    get_or_spawn_worker(app, state)?;

    let mut stdin_guard = state.child_stdin.lock().map_err(|err| err.to_string())?;
    let mut stdout_guard = state.child_stdout.lock().map_err(|err| err.to_string())?;

    let child_stdin = stdin_guard.as_mut().ok_or("Stdin is not initialized")?;
    let child_stdout = stdout_guard.as_mut().ok_or("Stdout is not initialized")?;

    let payload = serde_json::json!({
        "question": question,
        "previous_question": previous_question,
        "progress": progress.unwrap_or(&serde_json::Value::Null),
    });

    child_stdin
        .write_all((payload.to_string() + "\n").as_bytes())
        .map_err(|err| format!("Failed to write to Python worker: {err}"))?;
    child_stdin.flush().map_err(|err| format!("Failed to flush stdin: {err}"))?;

    let mut line1 = String::new();
    child_stdout
        .read_line(&mut line1)
        .map_err(|err| format!("Failed to read response line 1 from Python worker: {err}"))?;
    
    let trimmed1 = line1.trim();
    if trimmed1 == "__BLINKY_CAPTURED__" {
        if let Some(ref w) = command_window {
            set_window_capture_exclusion(w, false);
        }
        if let Some(ref w) = overlay_window {
            set_window_capture_exclusion(w, false);
        }
        
        let mut line2 = String::new();
        child_stdout
            .read_line(&mut line2)
            .map_err(|err| format!("Failed to read JSON response from Python worker: {err}"))?;
        Ok(line2.to_string())
    } else {
        Ok(line1.to_string())
    }
}


fn ensure_env_file(root: &PathBuf) {
    let env_path = root.join(".env");
    if !env_path.exists() {
        let example_path = root.join(".envexample");
        if example_path.exists() {
            let _ = std::fs::copy(&example_path, &env_path);
        } else {
            let _ = std::fs::write(&env_path, b"BLINKY_AI_PROVIDER=groq\nBLINKY_SHORTCUT=Space\n");
        }
    }
}

fn read_env_file(root: &PathBuf) -> Vec<(String, String)> {
    ensure_env_file(root);
    let env_path = root.join(".env");
    let Ok(contents) = std::fs::read_to_string(env_path) else {
        return Vec::new();
    };

    contents
        .lines()
        .filter_map(parse_env_line)
        .collect()
}

fn parse_env_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    Some((key.to_string(), trim_env_value(value)))
}

fn trim_env_value(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

#[allow(dead_code)]
fn parse_worker_error(stdout: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    parsed
        .get("error")
        .and_then(|error| error.as_str())
        .map(|error| error.to_string())
}

fn project_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("python").exists() {
            return Ok(cwd);
        }
        if let Some(parent) = cwd.parent() {
            if parent.join("python").exists() {
                return Ok(parent.to_path_buf());
            }
        }
    }

    app.path()
        .resource_dir()
        .map_err(|err| format!("Cannot locate app resource directory: {err}"))
}


fn python_executable(root: &PathBuf) -> PathBuf {
    let venv_python = root.join(".venv").join("Scripts").join("python.exe");
    if venv_python.exists() {
        venv_python
    } else {
        PathBuf::from("python")
    }
}

fn configure_overlay_passthrough(window: &WebviewWindow) {
    let _ = window.set_ignore_cursor_events(true);

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
            WS_EX_TRANSPARENT,
        };

        if let Ok(hwnd) = window.hwnd() {
            unsafe {
                let hwnd = hwnd.0 as HWND;
                let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
                SetWindowLongW(
                    hwnd,
                    GWL_EXSTYLE,
                    style | WS_EX_TRANSPARENT as i32 | WS_EX_LAYERED as i32 | WS_EX_TOOLWINDOW as i32,
                );
            }
        }
    }
}

fn set_window_capture_exclusion(window: &WebviewWindow, exclude: bool) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity;

        if let Ok(hwnd) = window.hwnd() {
            unsafe {
                let hwnd = hwnd.0 as HWND;
                let affinity = if exclude { 0x00000011 } else { 0x00000000 };
                let _ = SetWindowDisplayAffinity(hwnd, affinity);
            }
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            run_tutor,
            show_overlay,
            hide_overlay,
            show_command_bar,
            resize_command_window,
            resize_and_move_command_window,
            get_settings,
            save_settings,
            start_stt_stream,
            send_audio_chunk,
            stop_stt_stream
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show_command" => show_command_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .setup(|app| {
            app.manage(AppState {
                child: std::sync::Mutex::new(None),
                child_stdin: std::sync::Mutex::new(None),
                child_stdout: std::sync::Mutex::new(None),
                stt_tx: std::sync::Mutex::new(None),
            });

            setup_tray(app)?;

            if let Some(overlay) = app.get_webview_window("overlay") {
                configure_overlay_passthrough(&overlay);
            }

            if let Some(command) = app.get_webview_window("command") {
                let _ = command.show();
                let _ = command.set_focus();
            }

            let app_handle = app.handle().clone();
            start_global_click_listener(app_handle.clone());

            for code in [Code::Enter, Code::Space] {
                let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), code);
                let app_handle = app_handle.clone();
                if let Err(err) = app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let active = get_active_shortcut_from_env(&app_handle);
                        let is_match = match code {
                            Code::Enter => active == "Enter",
                            Code::Space => active == "Space",
                            _ => false,
                        };
                        if is_match {
                            show_command_window(&app_handle);
                        }
                    }
                }) {
                    eprintln!("Failed to register command shortcut {code:?}: {err}");
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Blinky");
}


fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show_command = MenuItem::with_id(app, "show_command", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Exit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_command, &quit])?;

    let mut tray = TrayIconBuilder::with_id("blinky")
        .tooltip("Blinky")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                show_command_window(app);
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    Ok(())
}

fn show_command_window(app: &AppHandle) {
    if let Some(command) = app.get_webview_window("command") {
        let _ = command.emit("blinky://open-command", ());
        let _ = command.unminimize();
        let _ = command.show();
        let _ = command.set_focus();
    }
}

fn start_global_click_listener(app: AppHandle) {
    thread::spawn(move || {
        let mut was_left_down = false;
        let mut was_right_down = false;
        let mut was_enter_down = false;

        loop {
            if let Some(click) = read_mouse_click(&mut was_left_down, &mut was_right_down) {
                if let Some(overlay) = app.get_webview_window("overlay") {
                    if overlay.is_visible().unwrap_or(false) {
                        let click = click.with_overlay_metrics(&overlay);
                        let _ = overlay.emit("blinky://global-click", click);
                    }
                }
            }

            if let Some(()) = read_enter_key(&mut was_enter_down) {
                let _ = app.emit("blinky://global-enter", ());
            }

            thread::sleep(Duration::from_millis(16));
        }
    });
}

impl GlobalClick {
    fn with_overlay_metrics(mut self, overlay: &WebviewWindow) -> Self {
        if let Ok(position) = overlay.outer_position() {
            self.overlay_x = position.x;
            self.overlay_y = position.y;
        }
        self.scale_factor = overlay.scale_factor().unwrap_or(1.0);
        self
    }
}

#[cfg(target_os = "windows")]
fn read_mouse_click(was_left_down: &mut bool, was_right_down: &mut bool) -> Option<GlobalClick> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let is_left_down = unsafe { (GetAsyncKeyState(VK_LBUTTON as i32) & 0x8000u16 as i16) != 0 };
    let is_right_down = unsafe { (GetAsyncKeyState(VK_RBUTTON as i32) & 0x8000u16 as i16) != 0 };
    let clicked = (is_left_down && !*was_left_down) || (is_right_down && !*was_right_down);
    *was_left_down = is_left_down;
    *was_right_down = is_right_down;

    if !clicked {
        return None;
    }

    let mut point = POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut point) };
    if ok == 0 {
        return None;
    }

    Some(GlobalClick {
        x: point.x,
        y: point.y,
        overlay_x: 0,
        overlay_y: 0,
        scale_factor: 1.0,
    })
}

#[cfg(not(target_os = "windows"))]
fn read_mouse_click(_was_left_down: &mut bool, _was_right_down: &mut bool) -> Option<GlobalClick> {
    None
}

#[cfg(target_os = "windows")]
fn read_enter_key(was_enter_down: &mut bool) -> Option<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_RETURN};

    let is_enter_down = unsafe { (GetAsyncKeyState(VK_RETURN as i32) & 0x8000u16 as i16) != 0 };
    let pressed = is_enter_down && !*was_enter_down;
    *was_enter_down = is_enter_down;

    if pressed {
        Some(())
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
fn read_enter_key(_was_enter_down: &mut bool) -> Option<()> {
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_env_line, parse_worker_error};

    #[test]
    fn parses_worker_json_error_from_stdout() {
        let stdout = r#"{"error":"Ollama is not running","steps":[],"warnings":[]}"#;

        assert_eq!(
            parse_worker_error(stdout),
            Some("Ollama is not running".to_string())
        );
    }

    #[test]
    fn ignores_non_json_worker_stdout() {
        assert_eq!(parse_worker_error("not json"), None);
    }

    #[test]
    fn parses_env_line_with_quoted_value() {
        assert_eq!(
            parse_env_line(r#"BLINKY_GROQ_MODEL="meta-llama/llama-4-scout-17b-16e-instruct""#),
            Some((
                "BLINKY_GROQ_MODEL".to_string(),
                "meta-llama/llama-4-scout-17b-16e-instruct".to_string()
            ))
        );
    }
}
