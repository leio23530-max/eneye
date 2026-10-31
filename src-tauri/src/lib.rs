mod agents;
mod capture;
mod dictionary;
mod fonts;
mod models;
mod settings;
mod storage;
mod translation;
mod word_probe;

use std::{
    path::PathBuf,
    sync::{Mutex, atomic::AtomicBool},
};

use base64::Engine as _;
use dictionary::{DictionaryManager, DictionaryStatus, ExternalDictionary};
use models::{BootstrapData, HistoryRecord, LookupResult, Notebook, NotebookWord};
use settings::{AppSettings, SettingsStore};
use storage::Storage;
use tauri::{
    Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tokio_util::sync::CancellationToken;
use translation::{AiKeyStore, TranslationEvent, TranslationRequest, TranslationTasks};

struct AppState {
    settings_store: SettingsStore,
    settings: Mutex<AppSettings>,
    storage: Mutex<Storage>,
    dictionary: Mutex<DictionaryManager>,
    translation_tasks: TranslationTasks,
    overlay_pinned: AtomicBool,
    last_word_probe: Mutex<word_probe::WordProbeReport>,
    overlay_mode: Mutex<capture::OverlayMode>,
    overlay_anchor: Mutex<Option<(i32, i32)>>,
    overlay_card_bounds: Mutex<Option<capture::OverlayBounds>>,
}

impl AppState {
    fn initialize(app: &tauri::App) -> anyhow::Result<Self> {
        let data_dir = app
            .path()
            .app_local_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."));
        std::fs::create_dir_all(&data_dir)?;

        let settings_store = SettingsStore::new(data_dir.join("settings.json"));
        let settings = settings_store.load().unwrap_or_default();
        let storage = Storage::open(&data_dir.join("history.sqlite"))?;
        let dictionary_path = ecdict_path(app)?;
        let dictionary =
            DictionaryManager::open(dictionary_path, data_dir.join("dictionaries.json"));

        Ok(Self {
            settings_store,
            settings: Mutex::new(settings),
            storage: Mutex::new(storage),
            dictionary: Mutex::new(dictionary),
            translation_tasks: TranslationTasks::default(),
            overlay_pinned: AtomicBool::new(false),
            last_word_probe: Mutex::new(word_probe::WordProbeReport::default()),
            overlay_mode: Mutex::new(capture::OverlayMode::Hidden),
            overlay_anchor: Mutex::new(None),
            overlay_card_bounds: Mutex::new(None),
        })
    }
}

fn ecdict_path(app: &tauri::App) -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("ENEYE_ECDICT_DB") {
        return Ok(PathBuf::from(path));
    }

    let bundled = app.path().resolve(
        "resources/ecdic.sqlite",
        tauri::path::BaseDirectory::Resource,
    )?;
    if bundled.exists() {
        return Ok(bundled);
    }

    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("ecdic.sqlite"))
}

#[tauri::command]
fn get_bootstrap(state: State<'_, AppState>) -> Result<BootstrapData, String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "settings lock is poisoned".to_owned())?
        .clone();
    let dictionary = state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .status();
    let external_dictionaries = state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .list_external();
    Ok(BootstrapData {
        settings,
        fonts: fonts::installed_fonts(),
        dictionary_status: dictionary,
        external_dictionaries,
        agent_connections: agents::detect(),
        ai_key_configured: AiKeyStore::exists(),
        ocr_available: capture::ocr_available(),
    })
}

#[tauri::command]
fn detect_agent_connections() -> Vec<agents::AgentConnectionStatus> {
    agents::detect()
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    if settings.launch_at_startup {
        app.autolaunch()
            .enable()
            .map_err(|error| error.to_string())?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|error| error.to_string())?;
    }
    let saved = state
        .settings_store
        .save(settings)
        .map_err(|error| error.to_string())?;
    *state
        .settings
        .lock()
        .map_err(|_| "settings lock is poisoned".to_owned())? = saved.clone();
    if let Ok(anchor) = state.overlay_anchor.lock()
        && let Some(point) = *anchor
    {
        capture::position_overlay(&app, point);
    }
    Ok(saved)
}

#[tauri::command]
fn lookup_word(
    word: String,
    source: Option<String>,
    state: State<'_, AppState>,
) -> Result<LookupResult, String> {
    let source = source.unwrap_or_else(|| "uia".into());
    let lookup = state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .lookup(&word, &source)
        .map_err(|error| error.to_string())?;
    if !lookup.entries.is_empty() {
        state
            .storage
            .lock()
            .map_err(|_| "storage lock is poisoned".to_owned())?
            .record_lookup(&lookup)
            .map_err(|error| error.to_string())?;
    }
    Ok(lookup)
}

#[tauri::command]
fn search_dictionary(query: String, state: State<'_, AppState>) -> Result<LookupResult, String> {
    state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .search(&query)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_history(
    query: Option<String>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<HistoryRecord>, String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .list(query.as_deref(), limit.unwrap_or(100))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_history(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .delete(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .clear()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_notebooks(state: State<'_, AppState>) -> Result<Vec<Notebook>, String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .list_notebooks()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_notebook(name: String, state: State<'_, AppState>) -> Result<Notebook, String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .create_notebook(&name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_notebook(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .delete_notebook(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_notebook_words(
    notebook_id: i64,
    query: Option<String>,
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<NotebookWord>, String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .list_notebook_words(notebook_id, query.as_deref(), limit.unwrap_or(200))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn add_word_to_notebook(
    notebook_id: i64,
    word: String,
    phonetic: String,
    definition: String,
    state: State<'_, AppState>,
) -> Result<NotebookWord, String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .add_word_to_notebook(notebook_id, &word, &phonetic, &definition)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_notebook_word(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    state
        .storage
        .lock()
        .map_err(|_| "storage lock is poisoned".to_owned())?
        .remove_notebook_word(id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_dictionary_status(state: State<'_, AppState>) -> Result<DictionaryStatus, String> {
    Ok(state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .status())
}

#[tauri::command]
fn get_capture_diagnostics(
    state: State<'_, AppState>,
) -> Result<word_probe::WordProbeReport, String> {
    state
        .last_word_probe
        .lock()
        .map(|report| report.clone())
        .map_err(|_| "word probe lock is poisoned".to_owned())
}

#[tauri::command]
fn list_external_dictionaries(
    state: State<'_, AppState>,
) -> Result<Vec<ExternalDictionary>, String> {
    Ok(state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .list_external())
}

#[tauri::command]
fn add_external_dictionary(
    path: String,
    state: State<'_, AppState>,
) -> Result<ExternalDictionary, String> {
    state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .add_external(PathBuf::from(path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_external_dictionary(id: String, state: State<'_, AppState>) -> Result<bool, String> {
    state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .remove_external(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reorder_external_dictionaries(
    ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .reorder_external(&ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_dictionary_resource(
    dictionary_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let bytes = state
        .dictionary
        .lock()
        .map_err(|_| "dictionary lock is poisoned".to_owned())?
        .resource(&dictionary_id, &path);
    Ok(bytes.map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command]
fn set_ai_key(api_key: String) -> Result<bool, String> {
    AiKeyStore::set(&api_key).map_err(|error| error.to_string())?;
    Ok(AiKeyStore::exists())
}

#[tauri::command]
fn delete_ai_key() -> Result<(), String> {
    AiKeyStore::set("").map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_ai_models(
    settings: AppSettings,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    let key = match api_key.filter(|key| !key.trim().is_empty()) {
        Some(key) => key,
        None => tokio::task::spawn_blocking(AiKeyStore::get)
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?,
    };
    translation::list_models(&settings, &key)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn translate_selection(
    app: tauri::AppHandle,
    request: TranslationRequest,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "settings lock is poisoned".to_owned())?
        .clone();
    if settings.translation_model().is_empty() {
        let message = "请先选择或填写 AI 模型名称".to_owned();
        let _ = app.emit(
            "eneye://translation",
            TranslationEvent::Failed {
                request_id: request.request_id.clone(),
                error: message.clone(),
            },
        );
        return Err(message);
    }
    let api_key = match tokio::task::spawn_blocking(AiKeyStore::get)
        .await
        .map_err(|error| error.to_string())?
    {
        Ok(api_key) => api_key,
        Err(error) => {
            let message = error.to_string();
            let _ = app.emit(
                "eneye://translation",
                TranslationEvent::Failed {
                    request_id: request.request_id.clone(),
                    error: message.clone(),
                },
            );
            return Err(message);
        }
    };
    let cancel = CancellationToken::new();
    state
        .translation_tasks
        .insert(request.request_id.clone(), cancel.clone())
        .map_err(|error| error.to_string())?;

    let request_id = request.request_id.clone();
    let source = request.source.clone();
    let original_text = request.text.clone();
    let event_app = app.clone();
    let result =
        translation::stream_translation(&settings, &api_key, &request, cancel, move |event| {
            let _ = event_app.emit("eneye://translation", event);
        })
        .await;
    state
        .translation_tasks
        .remove(&request_id)
        .map_err(|error| error.to_string())?;

    match result {
        Ok(outcome) => {
            if let Some(model) = outcome.unsupported_reasoning_model
                && let Ok(mut settings) = state.settings.lock()
                && settings.remember_unsupported_reasoning_model(&model)
            {
                match state.settings_store.save(settings.clone()) {
                    Ok(saved) => *settings = saved,
                    Err(error) => {
                        eprintln!("failed to remember reasoning support for {model}: {error}")
                    }
                }
            }
            state
                .storage
                .lock()
                .map_err(|_| "storage lock is poisoned".to_owned())?
                .record_translation(&original_text, &outcome.text, &source)
                .map_err(|error| error.to_string())?;
            Ok(())
        }
        Err(error) => {
            let message = error.to_string();
            if message != "translation cancelled" {
                let _ = app.emit(
                    "eneye://translation",
                    TranslationEvent::Failed {
                        request_id,
                        error: message.clone(),
                    },
                );
            }
            Err(message)
        }
    }
}

#[tauri::command]
fn cancel_translation(request_id: String, state: State<'_, AppState>) -> Result<bool, String> {
    state
        .translation_tasks
        .cancel(&request_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_overlay_pinned(pinned: bool, state: State<'_, AppState>) {
    state
        .overlay_pinned
        .store(pinned, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn set_overlay_card_bounds(bounds: Option<capture::OverlayBounds>, state: State<'_, AppState>) {
    if let Ok(mut current) = state.overlay_card_bounds.lock() {
        *current = bounds;
    }
}

#[tauri::command]
fn hide_overlay(app: tauri::AppHandle, state: State<'_, AppState>) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
    capture::mark_overlay_hidden();
    if let Ok(mut mode) = state.overlay_mode.lock() {
        *mode = capture::OverlayMode::Hidden;
    }
    if let Ok(mut anchor) = state.overlay_anchor.lock() {
        *anchor = None;
    }
    state
        .overlay_pinned
        .store(false, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn show_settings(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn create_overlay(app: &tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html".into()))
        .title("Eneye")
        .inner_size(440.0, 610.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .shadow(false)
        .build()?;
    Ok(())
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 Eneye", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("Eneye")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_settings(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn configure_shortcuts(app: &tauri::App) {
    if let Err(error) = app
        .global_shortcut()
        .on_shortcut("Ctrl+Alt+E", |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                show_settings(app.clone());
            }
        })
    {
        eprintln!("failed to register Ctrl+Alt+E: {error}");
    }
    if let Err(error) = app
        .global_shortcut()
        .on_shortcut("Ctrl+Alt+T", |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                capture::translate_focused_selection(app.clone());
            }
        })
    {
        eprintln!("failed to register Ctrl+Alt+T: {error}");
    }
}

fn dictionary_resource_response(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    use percent_encoding::percent_decode_str;
    use tauri::http::{StatusCode, header};

    let path = request.uri().path().trim_start_matches('/');
    let Some((dictionary_id, resource_path)) = path.split_once('/') else {
        return tauri::http::Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Vec::new())
            .unwrap();
    };
    let resource_path = percent_decode_str(resource_path)
        .decode_utf8_lossy()
        .replace('/', "\\");
    let state = ctx.app_handle().state::<AppState>();
    let resource = state
        .dictionary
        .lock()
        .ok()
        .and_then(|dictionary| dictionary.resource(dictionary_id, &resource_path));
    let Some(bytes) = resource else {
        return tauri::http::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap();
    };
    let content_type = resource_mime_type(resource_path.as_str());
    tauri::http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(bytes)
        .unwrap()
}

fn resource_mime_type(path: &str) -> &'static str {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "css" => "text/css; charset=utf-8",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .register_uri_scheme_protocol("eneye-dict", dictionary_resource_response)
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::initialize(app)?;
            if state
                .settings
                .lock()
                .map(|settings| settings.launch_at_startup)
                .unwrap_or(false)
            {
                let _ = app.autolaunch().enable();
            }
            app.manage(state);
            create_overlay(app)?;
            create_tray(app)?;
            configure_shortcuts(app);
            capture::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "settings"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                }
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            detect_agent_connections,
            save_settings,
            lookup_word,
            search_dictionary,
            get_history,
            delete_history,
            clear_history,
            list_notebooks,
            create_notebook,
            delete_notebook,
            list_notebook_words,
            add_word_to_notebook,
            remove_notebook_word,
            get_dictionary_status,
            get_capture_diagnostics,
            list_external_dictionaries,
            add_external_dictionary,
            remove_external_dictionary,
            reorder_external_dictionaries,
            get_dictionary_resource,
            set_ai_key,
            delete_ai_key,
            list_ai_models,
            translate_selection,
            cancel_translation,
            set_overlay_pinned,
            set_overlay_card_bounds,
            hide_overlay,
            show_settings
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Eneye");
}
