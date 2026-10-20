//! Transport-agnostic business operations shared by the Tauri desktop
//! commands and the headless HTTP server.
//!
//! Every operation mirrors one Tauri command: the desktop command wrappers
//! (`commands.rs`) and the server dispatcher both call these functions.
//! Argument names in the HTTP layer follow the Tauri convention (camelCase).

use crate::bus::Host;
use crate::database::{LiveRoom, RecordTask};
use crate::recorder::RecordingExit;
use crate::recording_log;
use crate::recovery::{self, RecoveryStatus};
use crate::settings::AppSettings;
use crate::{
    apply_live_info, auto_policy, parser::RequestSource, recovery_runtime, segment_runtime,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::PathBuf;

/// Extract a named argument with a clear error.
pub fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let value = args.get(name).ok_or_else(|| format!("缺少参数: {name}"))?;
    serde_json::from_value(value.clone()).map_err(|e| format!("参数 {name} 无效: {e}"))
}

pub fn optional<T: DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, String> {
    match args.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|e| format!("参数 {name} 无效: {e}")),
    }
}

fn to_value<T: serde::Serialize>(result: Result<T, String>) -> Result<Value, String> {
    result.and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string()))
}

pub async fn get_settings(host: &Host) -> Result<AppSettings, String> {
    let state = &host.state;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut current_settings = state.settings();
    current_settings.db_path = db.path().to_string_lossy().to_string();
    Ok(current_settings)
}

pub async fn save_settings(host: &Host, new_settings: AppSettings) -> Result<AppSettings, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let _start = state.start_lock.lock().await;
    persist_settings(host, new_settings)
}

/// Persist and apply settings without the async save locks. The caller must
/// hold `start_lock` and a lifecycle operation. The desktop command uses this
/// so it can sync OS autostart atomically with the settings write.
pub fn persist_settings(host: &Host, new_settings: AppSettings) -> Result<AppSettings, String> {
    let saved = crate::migration::save_settings(
        &host.state,
        new_settings,
        &crate::settings::settings_path(),
    )?;
    if !saved.recording_recovery_enabled {
        recovery_runtime::cancel_all(host, recovery::CancelReason::Settings, true);
    }
    host.state.lifecycle.close_to_tray.store(
        saved.close_behavior == crate::settings::CloseBehavior::Tray,
        std::sync::atomic::Ordering::Release,
    );
    Ok(saved)
}

pub async fn get_rooms(host: &Host) -> Result<Vec<LiveRoom>, String> {
    let db = host.state.db.lock().map_err(|e| e.to_string())?;
    db.get_all_rooms().map_err(|e| e.to_string())
}

pub async fn add_room(host: &Host, url: String) -> Result<LiveRoom, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let app_settings = state.settings();
    let info = state
        .parser
        .parse_douyin_url(&url, &app_settings, RequestSource::AddRoom)
        .await
        .map_err(|e| e.to_string())?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let room_id = db
        .add_room_full(
            &info.platform,
            &info.room_id,
            &info.anchor_name,
            &info.room_title,
            &info.cover_url,
            &info.avatar_url,
            info.is_live,
        )
        .map_err(|e| e.to_string())?;
    db.get_room(room_id).map_err(|e| e.to_string())
}

pub async fn refresh_room(host: &Host, room_id: i64) -> Result<LiveRoom, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    crate::refresh_room_internal(state, room_id).await
}

pub async fn set_room_auto_record(
    host: &Host,
    room_id: i64,
    enabled: bool,
) -> Result<LiveRoom, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let _start = state.start_lock.lock().await;
    if !enabled
        && state
            .recoveries
            .status(room_id)
            .is_some_and(|s| s.trigger == "auto")
    {
        recovery_runtime::cancel(host, room_id, None, recovery::CancelReason::Monitor, true);
    }
    let settings = state.settings();
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut room = db.get_room(room_id).map_err(|e| e.to_string())?;
    auto_policy::set_enabled(
        &mut room,
        enabled,
        settings.auto_monitor_window_hours,
        chrono::Utc::now(),
    );
    let room = db.save_room_automation(&room).map_err(|e| e.to_string())?;
    if enabled {
        state.auto_recorder.mark_immediate(room_id);
    } else {
        state.auto_recorder.clear(room_id);
    }
    crate::emit_auto_recording_event(
        host,
        room.clone(),
        if enabled { "enabled" } else { "disabled" },
        Some(
            if enabled {
                "自动录制已开启，正在检查直播状态"
            } else {
                "自动录制已关闭；正在进行的录制不会停止"
            }
            .to_string(),
        ),
    );
    Ok(room)
}

pub fn set_room_auto_config(
    host: &Host,
    room_id: i64,
    monitor_mode: auto_policy::AutoMonitorMode,
    daily_time: Option<String>,
) -> Result<LiveRoom, String> {
    crate::save_room_auto_config(host, room_id, Some(monitor_mode), daily_time)
}

pub fn set_room_auto_schedule(
    host: &Host,
    room_id: i64,
    daily_time: Option<String>,
) -> Result<LiveRoom, String> {
    crate::save_room_auto_config(host, room_id, None, daily_time)
}

pub async fn delete_room(host: &Host, id: i64, cascade: Option<bool>) -> Result<(), String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    {
        let _start = state.start_lock.lock().await;
        recovery_runtime::cancel(host, id, None, recovery::CancelReason::Superseded, true);
    }
    let _start = state.start_lock.lock().await;
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        if state.recoveries.busy(id)
            || db
                .has_running_tasks_for_room(id)
                .map_err(|e| e.to_string())?
        {
            return Err("该房间正在录制或结束处理中，请先停止录制".to_string());
        }
        if cascade == Some(true) {
            db.delete_room_cascade(id).map_err(|e| e.to_string())?;
        } else {
            let task_count = db.count_tasks_for_room(id).map_err(|e| e.to_string())?;
            if task_count > 0 {
                return Err(format!("该房间还有 {} 个录制任务，请确认删除", task_count));
            }
            db.delete_room(id).map_err(|e| e.to_string())?;
        }
    }
    state.auto_recorder.clear(id);
    Ok(())
}

pub fn get_room_task_count(host: &Host, room_id: i64) -> Result<i64, String> {
    let db = host.state.db.lock().map_err(|e| e.to_string())?;
    db.count_tasks_for_room(room_id).map_err(|e| e.to_string())
}

/// Newest-first history. `limit` bounds the page so the UI never renders the
/// entire history at once; omit it for internal callers that need everything.
pub async fn get_tasks(
    host: &Host,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<RecordTask>, String> {
    let db = host.state.db.lock().map_err(|e| e.to_string())?;
    match limit {
        Some(limit) => db
            .get_tasks_page(limit, offset.unwrap_or(0))
            .map_err(|e| e.to_string()),
        None => db.get_all_tasks().map_err(|e| e.to_string()),
    }
}

pub fn get_recording_recoveries(host: &Host) -> Result<Vec<RecoveryStatus>, String> {
    Ok(host.state.recoveries.snapshot())
}

pub async fn cancel_recording_recovery(
    host: &Host,
    room_id: i64,
    recovery_id: u64,
) -> Result<bool, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let _start = state.start_lock.lock().await;
    Ok(recovery_runtime::cancel(
        host,
        room_id,
        Some(recovery_id),
        recovery::CancelReason::User,
        false,
    )
    .is_some())
}

pub async fn start_record(host: &Host, room_id: i64) -> Result<RecordTask, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let previous = {
        let _start = state.start_lock.lock().await;
        if state.recoveries.busy(room_id) {
            if state
                .recoveries
                .status(room_id)
                .is_some_and(|s| s.phase == "recording")
            {
                return Err("该直播间已经在录制".into());
            }
            recovery_runtime::cancel(
                host,
                room_id,
                None,
                recovery::CancelReason::Superseded,
                true,
            )
            .map(|s| s.task_id)
        } else {
            None
        }
    };
    if let Some(task) = previous {
        state.recorder.stop_record(task).await?;
    }
    let douyin_url = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let room = db.get_room(room_id).map_err(|e| e.to_string())?;
        format!("https://live.douyin.com/{}", room.room_id)
    };
    let app_settings = state.settings();
    let info = state
        .parser
        .parse_douyin_url(&douyin_url, &app_settings, RequestSource::ManualStart)
        .await
        .map_err(|error| {
            recording_log::event("ERROR", "recording_start_failed", None, Some(room_id),
                json!({"trigger": "manual", "stage": "live_check", "error": error.to_string(), "rate_limited": error.is_rate_limited()}));
            error.to_string()
        })?;
    apply_live_info(state, room_id, &info)?;
    state.lifecycle.ensure_running()?;
    if !info.is_live {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let task_id = db.add_task(room_id, "manual").map_err(|e| e.to_string())?;
        db.update_task_status(task_id, "waiting")
            .map_err(|e| e.to_string())?;
        recording_log::event(
            "INFO",
            "recording_not_live",
            Some(task_id),
            Some(room_id),
            json!({"trigger": "manual"}),
        );
        return Err("主播未开播，任务已创建但未开始录制".to_string());
    }
    crate::start_record_from_info(host, room_id, &info, "manual", None)
        .await
        .map_err(|e| e.to_string())
}

pub async fn delete_task(host: &Host, id: i64) -> Result<(), String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    if state.recorder.is_active(id) {
        return Err("该任务正在录制或结束处理中，请先停止录制".to_string());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let task = db.get_task(id).map_err(|e| e.to_string())?;
    if task.status == "recording" || task.status == "finalizing" {
        return Err("该任务正在录制或结束处理中，请先停止录制".to_string());
    }
    if task
        .segments
        .iter()
        .any(|segment| matches!(segment.conversion_state.as_str(), "queued" | "converting"))
    {
        return Err("分段正在转换，请等待完成后再删除记录".into());
    }
    db.delete_task(id).map_err(|e| e.to_string())
}

pub async fn stop_record(host: &Host, task_id: i64) -> Result<RecordTask, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    {
        let _start_guard = state.start_lock.lock().await;
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let task = db.get_task(task_id).map_err(|e| e.to_string())?;
        let stopping_chain = state.recoveries.includes_task(task.room_id, task_id);
        state.recorder.request_stop(task_id);
        if stopping_chain {
            recovery_runtime::cancel(
                host,
                task.room_id,
                None,
                recovery::CancelReason::Stop,
                false,
            );
        }
        recording_log::event(
            "INFO",
            "recording_stop_requested",
            Some(task_id),
            Some(task.room_id),
            json!({"status": task.status}),
        );
        let mut room = db.get_room(task.room_id).map_err(|e| e.to_string())?;
        if auto_policy::disable_for_manual_stop(
            &mut room,
            if stopping_chain {
                "recording"
            } else {
                &task.status
            },
            chrono::Utc::now(),
        ) {
            let room = db.save_room_automation(&room).map_err(|e| e.to_string())?;
            state.auto_recorder.clear(room.id);
            crate::emit_auto_recording_event(
                host,
                room,
                "disabled",
                Some("已关闭持续监控，正在停止录制".to_string()),
            );
        }
    }
    let was_active = state.recorder.stop_record(task_id).await?;
    if !was_active {
        let task = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            db.get_task(task_id).map_err(|e| e.to_string())?
        };
        if task.status == "recording" {
            recording_log::event(
                "ERROR",
                "recording_process_missing",
                Some(task_id),
                Some(task.room_id),
                json!({"error": "未找到对应的 FFmpeg 进程"}),
            );
            crate::handle_recording_exit(
                host.clone(),
                task_id,
                task.room_id,
                RecordingExit {
                    manually_stopped: true,
                    status_success: false,
                    exit_code: None,
                    forced_stop_reason: None,
                    wait_error: Some("未找到对应的 FFmpeg 进程".to_string()),
                    stderr_tail: Vec::new(),
                },
            )
            .await?;
        }
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.get_task(task_id).map_err(|e| e.to_string())
}

pub async fn convert_to_mp4(host: &Host, task_id: i64) -> Result<String, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let task = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let task = db.get_task(task_id).map_err(|e| e.to_string())?;
        if task.status == "recording" || task.status == "finalizing" {
            return Err("录制或转换尚未结束，请稍后再试".into());
        }
        if task.segment_output.is_some() {
            return Err("请选择具体分段进行转换".into());
        }
        if task
            .file_path
            .as_deref()
            .is_none_or(|path| !path.ends_with(".flv"))
        {
            return Err("找不到可转换的 FLV 文件".into());
        }
        db.update_task_status(task_id, "finalizing")
            .map_err(|e| e.to_string())?;
        crate::emit_recording_event(
            host,
            db.get_task(task_id).map_err(|e| e.to_string())?,
            None,
            "conversion_started",
            None,
        );
        task
    };
    let source = task.file_path.clone().unwrap();
    let result = crate::conversion::remux(source.clone()).await;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    match &result {
        Ok((path, size)) => {
            db.finish_task(task_id, &task.status, Some(path), *size)
                .map_err(|e| e.to_string())?;
            if let Err(error) = std::fs::remove_file(&source) {
                crate::emit_recording_event(
                    host,
                    db.get_task(task_id).map_err(|e| e.to_string())?,
                    None,
                    "conversion_finished",
                    Some(format!("MP4 已保存，原 FLV 删除失败: {error}")),
                );
            }
        }
        Err(error) => {
            db.update_task_status(task_id, &task.status)
                .map_err(|e| e.to_string())?;
            state.lifecycle.record_failure(error.clone());
        }
    }
    crate::emit_recording_event(
        host,
        db.get_task(task_id).map_err(|e| e.to_string())?,
        None,
        "conversion_finished",
        None,
    );
    result.map(|(path, _)| path)
}

pub async fn convert_segment_to_mp4(host: &Host, segment_id: i64) -> Result<String, String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let task_id = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        if !db
            .claim_segment_conversion(segment_id)
            .map_err(|e| e.to_string())?
        {
            return Err("该分段正在录制、转换中，或没有可转换的 FLV 文件".into());
        }
        db.get_segment(segment_id)
            .map_err(|e| e.to_string())?
            .task_id
    };
    segment_runtime::emit_segments(host, task_id)?;
    let result = crate::conversion::convert_claimed_segment(host.clone(), segment_id).await;
    if let Err(error) = &result {
        state.lifecycle.record_failure(error.clone());
    }
    result
}

pub fn delete_segment(host: &Host, segment_id: i64) -> Result<(), String> {
    let state = &host.state;
    let _operation = state.lifecycle.operation()?;
    let task_id = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        if !db.hide_segment(segment_id).map_err(|e| e.to_string())? {
            return Err("正在录制或转换，暂时不能删除记录".into());
        }
        db.get_segment(segment_id)
            .map_err(|e| e.to_string())?
            .task_id
    };
    segment_runtime::emit_segments(host, task_id)?;
    Ok(())
}

pub async fn migrate_db(host: &Host, new_path: String) -> Result<String, String> {
    let state = host.state.clone();
    tokio::task::spawn_blocking(move || {
        let _operation = state.lifecycle.operation()?;
        crate::migration::migrate_database(&state, &new_path, &crate::settings::settings_path())
    })
    .await
    .map_err(|e| format!("数据库迁移任务异常: {e}"))?
}

/// The update check needs a package version; the headless server has no
/// package manifest, so it reports against a placeholder version.
pub async fn check_for_update(host: &Host) -> Result<Option<crate::updater::UpdateInfo>, String> {
    let app_settings = host.state.settings();
    let version = semver::Version::new(0, 1, 0);
    crate::updater::check_for_update(&version, &app_settings).await
}

/// Resolve the recorded media path for a task or segment.
fn resolve_media_path(
    host: &Host,
    task_id: Option<i64>,
    segment_id: Option<i64>,
) -> Result<(PathBuf, Option<String>), String> {
    let db = host.state.db.lock().map_err(|e| e.to_string())?;
    if let Some(segment_id) = segment_id {
        let segment = db
            .get_segment(segment_id)
            .map_err(|_| format!("分段 {segment_id} 不存在"))?;
        return Ok((
            PathBuf::from(&segment.file_path),
            segment.danmaku_path.clone(),
        ));
    }
    let task_id = task_id.ok_or_else(|| "缺少任务或分段参数".to_string())?;
    let task = db
        .get_task(task_id)
        .map_err(|_| format!("录制任务 {task_id} 不存在"))?;
    let media = task
        .file_path
        .clone()
        .ok_or_else(|| "该任务没有可用的录像文件".to_string())?;
    Ok((PathBuf::from(media), task.danmaku_path.clone()))
}

/// Convert the recorded danmaku XML into an ASS subtitle file next to the media
/// and return the new path.
pub fn export_danmaku_ass(
    host: &Host,
    task_id: Option<i64>,
    segment_id: Option<i64>,
) -> Result<String, String> {
    let (_, danmaku) = resolve_media_path(host, task_id, segment_id)?;
    let xml = danmaku.ok_or_else(|| "该录制没有弹幕文件".to_string())?;
    let ass = crate::danmaku::ass::export_ass_file(std::path::Path::new(&xml))?;
    Ok(ass.to_string_lossy().into_owned())
}

/// Escape a path for ffmpeg's filter argument (`ass=...`).
fn escape_filter_path(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len() + 8);
    for character in path.chars() {
        match character {
            '\\' | '\'' | ':' | '[' | ']' | ',' | ';' => {
                escaped.push('\\');
                escaped.push(character);
            }
            other => escaped.push(other),
        }
    }
    escaped
}

fn ffmpeg_has_libass(ffmpeg: &str) -> bool {
    let output = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-filters"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    let mut listing = String::from_utf8_lossy(&output.stdout).into_owned();
    listing.push_str(&String::from_utf8_lossy(&output.stderr));
    listing
        .lines()
        .any(|line| line.split_whitespace().any(|token| token == "ass"))
}

/// Serializes burn jobs: two concurrent burns of the same recording would write
/// the same output file.
static BURN_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Reject burning while the file is still being written.
fn ensure_not_recording(
    host: &Host,
    task_id: Option<i64>,
    segment_id: Option<i64>,
) -> Result<(), String> {
    let db = host.state.db.lock().map_err(|e| e.to_string())?;
    if let Some(segment_id) = segment_id {
        let segment = db
            .get_segment(segment_id)
            .map_err(|_| format!("分段 {segment_id} 不存在"))?;
        if segment.status == "recording" {
            return Err("该分段仍在录制，请停止后再烧录".into());
        }
        return Ok(());
    }
    if let Some(task_id) = task_id {
        let task = db
            .get_task(task_id)
            .map_err(|_| format!("录制任务 {task_id} 不存在"))?;
        if task.status == "recording" || task.status == "finalizing" {
            return Err("该录制仍在进行，请停止后再烧录".into());
        }
    }
    Ok(())
}

/// Convert user-supplied danmaku XML content into ASS text. The file is not
/// stored: callers either download the result or persist it explicitly.
pub fn convert_danmaku_xml(content: &str) -> Result<(String, usize), String> {
    if content.trim().is_empty() {
        return Err("弹幕文件内容为空".into());
    }
    crate::danmaku::ass::convert_text(content, &crate::danmaku::ass::AssConfig::default())
}

/// Persist converted ASS text into the recordings directory (desktop mode) and
/// return the final path. The file name is sanitized to a single component.
pub fn save_danmaku_ass(host: &Host, file_name: &str, content: &str) -> Result<String, String> {
    let safe = crate::segments::safe_filename(file_name);
    let stem = safe
        .strip_suffix(".ass")
        .or_else(|| safe.strip_suffix(".ASS"))
        .unwrap_or(&safe);
    let stem = if stem.is_empty() { "danmaku" } else { stem };
    let settings = host.state.settings();
    let directory = if settings.recordings_dir.is_empty() {
        crate::settings::default_db_dir().join("recordings")
    } else {
        std::path::PathBuf::from(&settings.recordings_dir)
    };
    std::fs::create_dir_all(&directory).map_err(|error| format!("创建录制目录失败: {error}"))?;
    let target = directory.join(format!("{stem}.ass"));
    std::fs::write(&target, content).map_err(|error| format!("写入字幕文件失败: {error}"))?;
    Ok(target.to_string_lossy().into_owned())
}

/// Burn the ASS subtitles into a new MP4 (`<name>_danmaku.mp4`). Re-encodes the
/// video, so it is slower than exporting the subtitle file.
pub async fn burn_danmaku(
    host: &Host,
    task_id: Option<i64>,
    segment_id: Option<i64>,
) -> Result<String, String> {
    ensure_not_recording(host, task_id, segment_id)?;
    let _burn_guard = BURN_LOCK.lock().await;
    let (media, danmaku) = resolve_media_path(host, task_id, segment_id)?;
    if !media.exists() {
        return Err(format!("录像文件不存在: {}", media.display()));
    }
    let xml = danmaku.ok_or_else(|| "该录制没有弹幕文件".to_string())?;
    let ffmpeg = crate::resolve_ffmpeg_path();
    if !ffmpeg_has_libass(&ffmpeg) {
        return Err(
            "当前 FFmpeg 未启用 libass，无法烧录字幕（可先导出 ASS 字幕用播放器加载）".into(),
        );
    }
    let ass = crate::danmaku::ass::export_ass_file(std::path::Path::new(&xml))?;
    let stem = media
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let output = media.with_file_name(format!("{stem}_danmaku.mp4"));
    let source = media.to_string_lossy().into_owned();
    let filter = format!("ass={}", escape_filter_path(&ass.to_string_lossy()));
    let target = output.to_string_lossy().into_owned();
    tokio::task::spawn_blocking(move || {
        let result = std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-i",
                &source,
                "-vf",
                &filter,
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "20",
                "-c:a",
                "copy",
                "-movflags",
                "+faststart",
                &target,
            ])
            .status()
            .map_err(|error| format!("启动 FFmpeg 失败: {error}"))?;
        if !result.success() {
            return Err(format!("烧录字幕失败（FFmpeg 退出码 {:?}）", result.code()));
        }
        match std::fs::metadata(&target) {
            Ok(metadata) if metadata.len() > 0 => Ok(target),
            _ => Err("烧录完成但输出文件为空".to_string()),
        }
    })
    .await
    .map_err(|error| format!("烧录任务异常: {error}"))?
}

/// Dispatch a command by name with JSON arguments, mirroring Tauri `invoke`.
pub async fn dispatch(host: &Host, command: &str, args: Value) -> Result<Value, String> {
    match command {
        "get_settings_cmd" => to_value(get_settings(host).await),
        "save_settings_cmd" => to_value(save_settings(host, arg(&args, "newSettings")?).await),
        "get_rooms" => to_value(get_rooms(host).await),
        "add_room" => to_value(add_room(host, arg(&args, "url")?).await),
        "refresh_room" => to_value(refresh_room(host, arg(&args, "roomId")?).await),
        "set_room_auto_record" => to_value(
            set_room_auto_record(host, arg(&args, "roomId")?, arg(&args, "enabled")?).await,
        ),
        "set_room_auto_schedule" => to_value(set_room_auto_schedule(
            host,
            arg(&args, "roomId")?,
            optional(&args, "dailyTime")?,
        )),
        "set_room_auto_config" => to_value(set_room_auto_config(
            host,
            arg(&args, "roomId")?,
            arg(&args, "monitorMode")?,
            optional(&args, "dailyTime")?,
        )),
        "delete_room" => {
            to_value(delete_room(host, arg(&args, "id")?, optional(&args, "cascade")?).await)
        }
        "get_room_task_count" => to_value(get_room_task_count(host, arg(&args, "roomId")?)),
        "get_tasks" => {
            to_value(get_tasks(host, optional(&args, "limit")?, optional(&args, "offset")?).await)
        }
        "delete_task" => to_value(delete_task(host, arg(&args, "id")?).await),
        "start_record" => to_value(start_record(host, arg(&args, "roomId")?).await),
        "stop_record" => to_value(stop_record(host, arg(&args, "taskId")?).await),
        "convert_to_mp4" => to_value(convert_to_mp4(host, arg(&args, "taskId")?).await),
        "convert_segment_to_mp4" => {
            to_value(convert_segment_to_mp4(host, arg(&args, "segmentId")?).await)
        }
        "delete_segment" => to_value(delete_segment(host, arg(&args, "segmentId")?)),
        "get_recording_recoveries" => to_value(get_recording_recoveries(host)),
        "cancel_recording_recovery" => to_value(
            cancel_recording_recovery(host, arg(&args, "roomId")?, arg(&args, "recoveryId")?).await,
        ),
        "convert_danmaku_xml" => {
            let content: String = arg(&args, "content")?;
            to_value(
                convert_danmaku_xml(&content)
                    .map(|(ass, count)| serde_json::json!({"ass": ass, "count": count})),
            )
        }
        "save_danmaku_ass" => to_value(save_danmaku_ass(
            host,
            &arg::<String>(&args, "fileName")?,
            &arg::<String>(&args, "content")?,
        )),
        "export_danmaku_ass" => to_value(export_danmaku_ass(
            host,
            optional(&args, "taskId")?,
            optional(&args, "segmentId")?,
        )),
        "burn_danmaku" => to_value(
            burn_danmaku(
                host,
                optional(&args, "taskId")?,
                optional(&args, "segmentId")?,
            )
            .await,
        ),
        "migrate_db_cmd" => to_value(migrate_db(host, arg(&args, "newPath")?).await),
        "check_for_update" => to_value(check_for_update(host).await),
        "get_recording_log_info" => to_value(recording_log::get_recording_log_info()),
        _ => Err(format!("未知命令: {command}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_extraction_reports_missing_and_invalid_values() {
        let args = json!({ "roomId": 7, "enabled": true, "dailyTime": null });
        assert_eq!(arg::<i64>(&args, "roomId").unwrap(), 7);
        assert!(arg::<bool>(&args, "enabled").unwrap());
        assert_eq!(optional::<String>(&args, "dailyTime").unwrap(), None);
        let missing = arg::<i64>(&args, "taskId").unwrap_err();
        assert!(missing.contains("缺少参数: taskId"));
        let invalid = arg::<i64>(&args, "enabled").unwrap_err();
        assert!(invalid.contains("参数 enabled 无效"));
    }

    #[test]
    fn uploaded_xml_converts_to_ass_text() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<i>
<d p="1.000,1,25,16777215,1760341234567,0,42,0" user="甲">你好</d>
<d p="2.000,5,25,16711680,1760341234999,0,43,0" user="乙">顶部</d>
</i>"#;
        let (ass, count) = convert_danmaku_xml(xml).unwrap();
        assert_eq!(count, 2);
        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("\\move(1920,"));
        assert!(ass.contains("\\an8\\pos(960,"));
        assert!(convert_danmaku_xml("   ").is_err());
    }

    #[tokio::test]
    async fn unknown_command_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = crate::Runtime::with_db_path(&dir.path().join("db.sqlite")).unwrap();
        let result = dispatch(&runtime.host, "not_a_command", json!({})).await;
        assert!(result.unwrap_err().contains("未知命令"));
    }

    #[test]
    fn host_construction_uses_state_bus() {
        let runtime = crate::Runtime::with_db_path(std::path::Path::new(
            &tempfile::tempdir().unwrap().path().join("db.sqlite"),
        ))
        .unwrap();
        let host = runtime.host.clone();
        let mut rx = host.bus.subscribe();
        host.emit("ping", serde_json::json!({"n": 1}));
        assert_eq!(rx.try_recv().unwrap().0, "ping");
    }
}
