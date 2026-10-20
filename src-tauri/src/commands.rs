//! Tauri command wrappers (desktop build only).
//!
//! Every command is a thin shell: it extracts the shared state, builds a
//! `Host` and calls the same `service` function the headless HTTP server uses.
//! Keeping the bodies here (and out of `lib.rs`) means the core library has no
//! compile-time or runtime dependency on Tauri when the `server` feature is
//! selected.

use std::sync::Arc;

use tauri::{Manager, State};

use douyin_recorder_core::bus::Host;
use douyin_recorder_core::database::{LiveRoom, RecordTask};
use douyin_recorder_core::recovery;
use douyin_recorder_core::settings::AppSettings;
use douyin_recorder_core::{recording_log, service, AppState};

fn host(state: &State<'_, Arc<AppState>>) -> Host {
    Host::new(state.inner().clone())
}

#[tauri::command]
pub async fn get_settings_cmd(state: State<'_, Arc<AppState>>) -> Result<AppSettings, String> {
    service::get_settings(&host(&state)).await
}

#[tauri::command]
pub async fn save_settings_cmd(
    state: State<'_, Arc<AppState>>,
    new_settings: AppSettings,
) -> Result<AppSettings, String> {
    service::save_settings(&host(&state), new_settings).await
}

#[tauri::command]
pub async fn migrate_db_cmd(
    state: State<'_, Arc<AppState>>,
    new_path: String,
) -> Result<String, String> {
    service::migrate_db(&host(&state), new_path).await
}

#[tauri::command]
pub async fn check_for_update(
    app: tauri::AppHandle,
) -> Result<Option<douyin_recorder_core::updater::UpdateInfo>, String> {
    let current_version = app.package_info().version.clone();
    let app_settings =
        service::get_settings(&Host::new(app.state::<Arc<AppState>>().inner().clone())).await?;
    douyin_recorder_core::updater::check_for_update(&current_version, &app_settings).await
}

#[tauri::command]
pub async fn get_rooms(state: State<'_, Arc<AppState>>) -> Result<Vec<LiveRoom>, String> {
    service::get_rooms(&host(&state)).await
}

#[tauri::command]
pub async fn add_room(state: State<'_, Arc<AppState>>, url: String) -> Result<LiveRoom, String> {
    service::add_room(&host(&state), url).await
}

#[tauri::command]
pub async fn refresh_room(
    state: State<'_, Arc<AppState>>,
    room_id: i64,
) -> Result<LiveRoom, String> {
    service::refresh_room(&host(&state), room_id).await
}

#[tauri::command]
pub async fn set_room_auto_record(
    state: State<'_, Arc<AppState>>,
    room_id: i64,
    enabled: bool,
) -> Result<LiveRoom, String> {
    service::set_room_auto_record(&host(&state), room_id, enabled).await
}

#[tauri::command]
pub fn set_room_auto_config(
    state: State<'_, Arc<AppState>>,
    room_id: i64,
    monitor_mode: douyin_recorder_core::auto_policy::AutoMonitorMode,
    daily_time: Option<String>,
) -> Result<LiveRoom, String> {
    service::set_room_auto_config(&host(&state), room_id, monitor_mode, daily_time)
}

#[tauri::command]
pub fn set_room_auto_schedule(
    state: State<'_, Arc<AppState>>,
    room_id: i64,
    daily_time: Option<String>,
) -> Result<LiveRoom, String> {
    service::set_room_auto_schedule(&host(&state), room_id, daily_time)
}

#[tauri::command]
pub fn get_room_task_count(state: State<'_, Arc<AppState>>, room_id: i64) -> Result<i64, String> {
    service::get_room_task_count(&host(&state), room_id)
}

#[tauri::command]
pub async fn delete_room(
    state: State<'_, Arc<AppState>>,
    id: i64,
    cascade: Option<bool>,
) -> Result<(), String> {
    service::delete_room(&host(&state), id, cascade).await
}

#[tauri::command]
pub async fn get_tasks(
    state: State<'_, Arc<AppState>>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<RecordTask>, String> {
    service::get_tasks(&host(&state), limit, offset).await
}

#[tauri::command]
pub fn get_recording_recoveries(state: State<'_, Arc<AppState>>) -> Vec<recovery::RecoveryStatus> {
    state.recoveries.snapshot()
}

#[tauri::command]
pub async fn cancel_recording_recovery(
    state: State<'_, Arc<AppState>>,
    room_id: i64,
    recovery_id: u64,
) -> Result<bool, String> {
    service::cancel_recording_recovery(&host(&state), room_id, recovery_id).await
}

#[tauri::command]
pub async fn start_record(
    state: State<'_, Arc<AppState>>,
    room_id: i64,
) -> Result<RecordTask, String> {
    service::start_record(&host(&state), room_id).await
}

#[tauri::command]
pub async fn delete_task(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    service::delete_task(&host(&state), id).await
}

#[tauri::command]
pub async fn stop_record(
    state: State<'_, Arc<AppState>>,
    task_id: i64,
) -> Result<RecordTask, String> {
    service::stop_record(&host(&state), task_id).await
}

#[tauri::command]
pub async fn convert_to_mp4(
    state: State<'_, Arc<AppState>>,
    task_id: i64,
) -> Result<String, String> {
    service::convert_to_mp4(&host(&state), task_id).await
}

#[tauri::command]
pub async fn convert_segment_to_mp4(
    state: State<'_, Arc<AppState>>,
    segment_id: i64,
) -> Result<String, String> {
    service::convert_segment_to_mp4(&host(&state), segment_id).await
}

#[tauri::command]
pub fn delete_segment(state: State<'_, Arc<AppState>>, segment_id: i64) -> Result<(), String> {
    service::delete_segment(&host(&state), segment_id)
}

#[tauri::command]
pub fn convert_danmaku_xml(content: String) -> Result<serde_json::Value, String> {
    service::convert_danmaku_xml(&content)
        .map(|(ass, count)| serde_json::json!({ "ass": ass, "count": count }))
}

#[tauri::command]
pub fn save_danmaku_ass(
    state: State<'_, Arc<AppState>>,
    file_name: String,
    content: String,
) -> Result<String, String> {
    service::save_danmaku_ass(&host(&state), &file_name, &content)
}

#[tauri::command]
pub fn export_danmaku_ass(
    state: State<'_, Arc<AppState>>,
    task_id: Option<i64>,
    segment_id: Option<i64>,
) -> Result<String, String> {
    service::export_danmaku_ass(&host(&state), task_id, segment_id)
}

#[tauri::command]
pub async fn burn_danmaku(
    state: State<'_, Arc<AppState>>,
    task_id: Option<i64>,
    segment_id: Option<i64>,
) -> Result<String, String> {
    service::burn_danmaku(&host(&state), task_id, segment_id).await
}

#[tauri::command]
pub fn get_recording_log_info() -> Result<recording_log::RecordingLogInfo, String> {
    recording_log::get_recording_log_info()
}

/// Desktop-only lifecycle state (tray/window exit coordination).
#[tauri::command]
pub fn get_lifecycle_status(
    state: State<'_, Arc<AppState>>,
) -> douyin_recorder_core::lifecycle::LifecycleStatus {
    state.lifecycle.status()
}
