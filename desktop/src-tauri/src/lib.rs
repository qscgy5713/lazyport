use serde::Serialize;
use std::sync::Mutex;
use std::{fs, path::PathBuf};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+P";

/// 目前註冊中的快捷鍵(字串形式,同時也是存檔格式)
struct Hotkey(Mutex<String>);

#[derive(Serialize)]
struct PortInfo {
    port: u16,
    pid: u32,
    command: String,
    addr: String,
}

impl From<lazyport::Listener> for PortInfo {
    fn from(l: lazyport::Listener) -> Self {
        Self {
            port: l.port,
            pid: l.pid,
            command: l.command,
            addr: l.addr,
        }
    }
}

#[derive(Serialize)]
struct ProcessDetail {
    pid: u32,
    user: String,
    started: String,
    exe: String,
    args: String,
    cwd: String,
    hint: Option<String>,
    custom_id: Option<String>,
    cwd_generic: bool,
    advice: String,
    advice_label: String,
    advice_note: String,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("settings.json"))
        .map_err(|e| e.to_string())
}

fn load_shortcut(app: &AppHandle) -> String {
    settings_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("shortcut")?.as_str().map(String::from))
        .filter(|s| s.parse::<Shortcut>().is_ok())
        .unwrap_or_else(|| DEFAULT_SHORTCUT.to_string())
}

fn save_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("無法建立設定資料夾: {e}"))?;
    }
    fs::write(path, serde_json::json!({ "shortcut": shortcut }).to_string())
        .map_err(|e| format!("無法儲存設定: {e}"))
}

// lsof / kill 會阻塞,放到 blocking 執行緒避免卡住 UI
#[tauri::command]
async fn list_ports() -> Result<Vec<PortInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        lazyport::listeners().map(|v| v.into_iter().map(PortInfo::from).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

// 只接受 port,由核心層重新查出實際監聽的 pid 才發送訊號,前端無法指定任意 pid
#[tauri::command]
async fn kill_port(port: u16, force: bool) -> Result<Vec<PortInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        lazyport::kill_port(port, force).map(|v| v.into_iter().map(PortInfo::from).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

// 唯讀:核心層只回答「目前正在監聽 port」的 pid
#[tauri::command]
async fn process_info(pid: u32) -> Result<ProcessDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        lazyport::process_info(pid).map(|i| ProcessDetail {
            pid: i.pid,
            user: i.user,
            started: i.started,
            exe: i.exe,
            args: i.args,
            cwd: i.cwd,
            hint: i.hint,
            custom_id: i.custom_id,
            cwd_generic: i.cwd_generic,
            advice: i.advice.as_str().to_string(),
            advice_label: i.advice.label().to_string(),
            advice_note: i.advice_note,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

// 自訂說明:輸入驗證與存檔都在核心層(custom.rs),這裡只轉接
#[tauri::command]
fn list_custom_hints() -> Result<Vec<lazyport::custom::Rule>, String> {
    lazyport::custom::list_rules()
}

#[tauri::command]
fn save_custom_hint(
    id: Option<String>,
    kind: lazyport::custom::MatchKind,
    pattern: String,
    desc: String,
    advice: lazyport::Advice,
    note: String,
) -> Result<lazyport::custom::Rule, String> {
    lazyport::custom::save_rule(id, kind, &pattern, &desc, advice, &note)
}

#[tauri::command]
fn delete_custom_hint(id: String) -> Result<(), String> {
    lazyport::custom::delete_rule(&id)
}

#[tauri::command]
fn get_shortcut(state: State<Hotkey>) -> String {
    state.0.lock().unwrap().clone()
}

// 先註冊新的、存檔成功後才取消舊的,任何一步失敗都維持原本的快捷鍵
#[tauri::command]
fn set_shortcut(app: AppHandle, state: State<Hotkey>, shortcut: String) -> Result<String, String> {
    let new: Shortcut = shortcut.parse().map_err(|e| format!("無效的快捷鍵: {e}"))?;
    let mut current = state.0.lock().unwrap();
    if *current == shortcut {
        return Ok(shortcut);
    }
    let gs = app.global_shortcut();
    gs.register(new)
        .map_err(|e| format!("無法註冊,可能已被其他程式佔用: {e}"))?;
    if let Err(e) = save_shortcut(&app, &shortcut) {
        let _ = gs.unregister(new);
        return Err(e);
    }
    if let Ok(old) = current.parse::<Shortcut>() {
        let _ = gs.unregister(old);
    }
    *current = shortcut.clone();
    Ok(shortcut)
}

// 顯示視窗並跳到最前面。選單列程式沒有 Dock 圖示,啟動時若不主動前置,
// 視窗會被其他 App 的視窗擋在後面,使用者會以為「打開後什麼都沒有」
fn show_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

// 視窗在前景時隱藏,否則顯示並取得焦點
fn toggle_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_window(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            list_ports,
            kill_port,
            process_info,
            list_custom_hints,
            save_custom_hint,
            delete_custom_hint,
            get_shortcut,
            set_shortcut
        ])
        .setup(|app| {
            // 選單列常駐:不顯示 Dock 圖示
            #[cfg(target_os = "macos")]
            app.set_activation_policy(ActivationPolicy::Accessory);

            let toggle = MenuItem::with_id(app, "toggle", "顯示 / 隱藏", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "結束 lazyport", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle, &quit])?;

            // 單色 template 圖示:macOS 會依淺色 / 深色選單列自動反白
            TrayIconBuilder::new()
                .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip("lazyport")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "toggle" => toggle_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // 讀取使用者自訂的快捷鍵;被其他程式佔用時只提示,不讓 App 啟動失敗
            let initial = load_shortcut(app.handle());
            if let Ok(sc) = initial.parse::<Shortcut>() {
                if let Err(e) = app.global_shortcut().register(sc) {
                    eprintln!("無法註冊快捷鍵 {initial}: {e}");
                }
            }
            app.manage(Hotkey(Mutex::new(initial)));

            show_window(app.handle());
            Ok(())
        })
        // 關閉視窗改為隱藏,App 繼續留在選單列
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("啟動 lazyport 桌面版失敗")
        .run(|app, event| {
            // 已在執行時再次點擊 App 圖示(Finder、Spotlight、Launchpad)→ 叫出視窗
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_window(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
