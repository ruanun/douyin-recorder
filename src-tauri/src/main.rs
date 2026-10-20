//! Desktop entry point: a thin Tauri shell around the shared core runtime.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod commands;
mod desktop;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use douyin_recorder_core::bus::Host;
use douyin_recorder_core::{AppState, Runtime};
use tauri::{Emitter, Manager};

fn main() {
    tauri::Builder::default()
        // This must run before database reconciliation or any background task.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            desktop::request_restore(app)
        }))
        .plugin(tauri_plugin_opener::init())
        // Autostart starts the app with --hidden so it lives in the tray.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let mut runtime = Runtime::new().map_err(std::io::Error::other)?;
            let host: Host = runtime.host.clone();
            let state: Arc<AppState> = host.state.clone();
            app.manage(state);
            // Forward core bus events to the webview.
            {
                let bus = host.bus.clone();
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut rx = bus.subscribe();
                    while let Ok((event, payload)) = rx.recv().await {
                        let _ = app_handle.emit(&event, payload);
                    }
                });
            }
            // Keep the system login item in sync with the saved preference.
            {
                use tauri_plugin_autostart::ManagerExt;
                let app_settings = douyin_recorder_core::settings::load_settings();
                if let Err(error) =
                    autostart::sync_on_startup(&*app.autolaunch(), app_settings.launch_on_startup)
                {
                    desktop::report_error(app.handle(), format!("同步系统自启动设置失败: {error}"));
                }
            }
            // A hidden launch (autostart) goes straight to the tray; otherwise
            // show the window (it starts hidden so the first frame is not blank).
            let launched_hidden = std::env::args().any(|argument| argument == "--hidden");
            let presentation = if launched_hidden {
                desktop::enter_tray_at_startup(app.handle())
            } else {
                desktop::restore_window(app.handle())
            };
            if let Err(error) = presentation {
                desktop::report_error(app.handle(), format!("初始化窗口或托盘失败: {error}"));
            }
            // Monitoring loop + danmaku status forwarding run on Tauri's
            // async runtime (the setup hook has no ambient tokio runtime).
            let (monitor, danmaku_forwarder) = runtime.background();
            tauri::async_runtime::spawn(monitor);
            tauri::async_runtime::spawn(danmaku_forwarder);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    desktop::request_close(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_rooms,
            commands::add_room,
            commands::refresh_room,
            commands::set_room_auto_record,
            commands::set_room_auto_schedule,
            commands::set_room_auto_config,
            commands::delete_room,
            commands::get_room_task_count,
            commands::get_tasks,
            commands::delete_task,
            commands::start_record,
            commands::stop_record,
            commands::convert_to_mp4,
            commands::convert_segment_to_mp4,
            commands::delete_segment,
            commands::get_settings_cmd,
            commands::save_settings_cmd,
            commands::get_recording_recoveries,
            commands::cancel_recording_recovery,
            commands::migrate_db_cmd,
            commands::check_for_update,
            commands::get_lifecycle_status,
            commands::get_recording_log_info,
            autostart::is_autostart_available,
            commands::convert_danmaku_xml,
            commands::save_danmaku_ass,
            commands::export_danmaku_ass,
            commands::burn_danmaku,
        ])
        .build(tauri::generate_context!())
        .expect("应用启动失败")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    if !state.lifecycle.allow_exit.load(Ordering::Acquire) {
                        api.prevent_exit();
                        desktop::request_exit(app);
                    }
                }
            }
        });
}
