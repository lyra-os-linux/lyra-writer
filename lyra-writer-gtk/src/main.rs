#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Lyra Writer — protótipo. Mesmo esqueleto do vega-gtk: Tauri 2 com
//! WebKitGTK, barra de título própria, tema e cor de destaque do GNOME.
//!
//! A GRAVAÇÃO É SIMULADA: nenhum byte vai para o dispositivo. A versão final
//! abre o bloco pelo udisks2 (`OpenDevice`, autorizado pelo polkit) em vez de
//! rodar como root.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use gio::prelude::*;
use lyra_writer_core::{Device, Image};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

const INTERFACE: &str = "org.gnome.desktop.interface";

/// Contorno do crash do WebKitGTK 2.52 ao encerrar (vega#166).
fn webkit_env() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: início do main, uma única thread.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
}

fn interface_schema() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(INTERFACE, true)?;
    Some(gio::Settings::new(INTERFACE))
}

fn interface_state(settings: &gio::Settings) -> Value {
    let accent = settings
        .settings_schema()
        .filter(|schema| schema.has_key("accent-color"))
        .map(|_| settings.string("accent-color").to_string());
    json!({ "color_scheme": settings.string("color-scheme").as_str(), "accent": accent })
}

#[tauri::command]
fn app_interface() -> Value {
    interface_schema()
        .map(|s| interface_state(&s))
        .unwrap_or_else(|| json!({ "color_scheme": "default", "accent": "blue" }))
}

/// Emite `interface-changed` quando o tema ou a cor de destaque mudam.
fn watch_interface(app: AppHandle) {
    let Some(settings) = interface_schema() else {
        return;
    };
    settings.connect_changed(None, move |settings, key| {
        if key == "color-scheme" || key == "accent-color" {
            let _ = app.emit("interface-changed", interface_state(settings));
        }
    });
    let _ = settings.string("color-scheme");
    if settings.settings_schema().is_some_and(|s| s.has_key("accent-color")) {
        let _ = settings.string("accent-color");
    }
    std::mem::forget(settings);
}

#[tauri::command]
fn app_log(message: String) {
    eprintln!("lyra-writer[ui]: {message}");
}

#[tauri::command]
fn images() -> Vec<Image> {
    lyra_writer_core::images()
}

#[tauri::command]
async fn devices() -> Result<Vec<Device>, String> {
    tauri::async_runtime::spawn_blocking(lyra_writer_core::devices)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Não foi possível consultar o udisks2: {e}"))
}

/// Conectar ou remover um pendrive atualiza a lista sozinho. O protótipo
/// consulta o udisks2 a cada 1,5 s; a versão final pode escutar os sinais
/// InterfacesAdded/InterfacesRemoved.
fn watch_devices(app: AppHandle) {
    thread::spawn(move || {
        let mut last: Option<Vec<Device>> = None;
        loop {
            if let Ok(now) = lyra_writer_core::devices() {
                if last.as_ref() != Some(&now) {
                    let _ = app.emit("devices-changed", &now);
                    last = Some(now);
                }
            }
            thread::sleep(Duration::from_millis(1500));
        }
    });
}

#[derive(Default)]
struct Writing {
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
struct Progress {
    phase: &'static str,
    percent: u8,
}

/// Fases e a fatia da barra que cada uma ocupa.
const WRITE: &[(&str, u8, u8)] = &[
    ("download", 0, 25),
    ("signature", 25, 35),
    ("write", 35, 85),
    ("verify", 85, 100),
];

/// Restaurar, como no Fedora Media Writer: apagar a imagem, criar uma
/// partição única e formatar para uso comum (exFAT).
const RESTORE: &[(&str, u8, u8)] = &[("wipe", 0, 40), ("partition", 40, 60), ("format", 60, 100)];

#[tauri::command]
fn write_start(app: AppHandle, state: State<Writing>, image: String, device: String) {
    eprintln!("lyra-writer: SIMULAÇÃO de gravação de {image} em {device}");
    run(app, &state, WRITE, 90);
}

#[tauri::command]
fn restore_start(app: AppHandle, state: State<Writing>, device: String) {
    eprintln!("lyra-writer: SIMULAÇÃO de restauração de {device}");
    run(app, &state, RESTORE, 45);
}

fn run(app: AppHandle, state: &Writing, phases: &'static [(&'static str, u8, u8)], step_ms: u64) {
    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    thread::spawn(move || {
        for &(phase, from, to) in phases {
            for percent in from..to {
                if cancel.load(Ordering::Relaxed) {
                    let _ = app.emit("write-cancelled", ());
                    return;
                }
                let _ = app.emit("write-progress", Progress { phase, percent });
                thread::sleep(Duration::from_millis(step_ms));
            }
        }
        let _ = app.emit("write-done", ());
    });
}

#[tauri::command]
fn write_cancel(state: State<Writing>) {
    state.cancel.store(true, Ordering::Relaxed);
}

fn main() {
    webkit_env();
    glib::set_prgname(Some("org.lyraos.Writer"));
    glib::set_application_name("Lyra Writer");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(Writing::default())
        .setup(|app| {
            watch_interface(app.handle().clone());
            watch_devices(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_interface,
            app_log,
            images,
            devices,
            write_start,
            restore_start,
            write_cancel
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o Lyra Writer");
}
