use crate::bus::Host;
use crate::segments::{ManifestReader, SegmentOutput};
use tokio::sync::watch;

pub fn emit_segments(host: &Host, task_id: i64) -> Result<(), String> {
    let state = host.state.clone();
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let task = db.get_task(task_id).map_err(|e| e.to_string())?;
    // Publishing under the DB lock keeps successive snapshots ordered.
    host.emit("recording-segments-changed", task);
    Ok(())
}

pub async fn monitor(
    host: Host,
    task_id: i64,
    output: SegmentOutput,
    mut stop: watch::Receiver<bool>,
    work: std::sync::Arc<crate::lifecycle::Operation>,
) -> Result<(), String> {
    let mut reader = ManifestReader::default();
    let mut entries = vec![];
    let mut last_snapshot = String::new();
    let failure = loop {
        let stopped = *stop.borrow() || stop.has_changed().is_err();
        let observation = (|| -> Result<Vec<i64>, String> {
            entries.extend(reader.read(&output)?);
            let state = host.state.clone();
            let db = state.db.lock().map_err(|e| e.to_string())?;
            db.observe_segments(task_id, &output, &entries)
                .map_err(|e| e.to_string())?;
            // Rotate the danmaku session to follow the active video segment:
            // each part gets its own XML with timestamps relative to its
            // start, while the WebSocket stays connected.
            let active_segment = db
                .get_segments(task_id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|segment| segment.status == "recording")
                .max_by_key(|segment| segment.segment_index);
            if let Some(segment) = active_segment {
                let danmaku_path =
                    std::path::PathBuf::from(&segment.file_path).with_extension("xml");
                let start_time = crate::parse_task_start_time(&segment.start_time);
                if state.danmaku.rotate(
                    task_id,
                    segment.segment_index,
                    danmaku_path.clone(),
                    start_time,
                ) {
                    db.set_segment_danmaku(segment.id, &danmaku_path.to_string_lossy())
                        .map_err(|e| e.to_string())?;
                }
            }
            let mut ids = vec![];
            if state.settings().auto_convert_mp4 {
                for segment in db.get_segments(task_id).map_err(|e| e.to_string())? {
                    if segment.status == "completed"
                        && segment.conversion_state == "idle"
                        && db
                            .claim_segment_conversion(segment.id)
                            .map_err(|e| e.to_string())?
                    {
                        ids.push(segment.id);
                    }
                }
            }
            let task = db.get_task(task_id).map_err(|e| e.to_string())?;
            let snapshot = serde_json::to_string(&task).map_err(|e| e.to_string())?;
            if snapshot != last_snapshot {
                host.emit("recording-segments-changed", task);
                last_snapshot = snapshot;
            }
            Ok(ids)
        })();
        let failure = match observation {
            Ok(ids) => {
                for id in ids {
                    let host = host.clone();
                    queue_conversion(host, id, work.clone());
                }
                None
            }
            Err(error) => Some(error),
        };
        if stopped {
            break failure;
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {},
            _ = stop.changed() => {},
        }
    };
    // Output observation ends immediately; registered conversions keep their own
    // lifecycle guard so exit still waits, without holding up the next capture.
    failure.map_or(Ok(()), Err)
}

pub async fn finish_segments(
    host: &Host,
    task_id: i64,
    status: &str,
    work: Option<std::sync::Arc<crate::lifecycle::Operation>>,
) -> Result<(), String> {
    let state = host.state.clone();
    let ids = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let task = db.get_task(task_id).map_err(|e| e.to_string())?;
        let output = task.segment_output.as_ref().ok_or("找不到分段设置")?;
        let entries = ManifestReader::read_all(output)?;
        db.observe_segments(task_id, output, &entries)
            .map_err(|e| e.to_string())?;
        let mut ids = vec![];
        for segment in db.get_segments(task_id).map_err(|e| e.to_string())? {
            if segment.status == "recording" {
                let end = entries
                    .iter()
                    .find(|entry| entry.index == segment.segment_index)
                    .map(|entry| crate::segments::segment_time(&task.start_time, entry.end))
                    .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string());
                db.close_segment(segment.id, status, &end)
                    .map_err(|e| e.to_string())?;
            }
            let updated = db.get_segment(segment.id).map_err(|e| e.to_string())?;
            if state.settings().auto_convert_mp4
                && updated.status == "completed"
                && updated.conversion_state == "idle"
                && db
                    .claim_segment_conversion(segment.id)
                    .map_err(|e| e.to_string())?
            {
                ids.push(segment.id);
            }
        }
        ids
    };
    emit_segments(host, task_id)?;
    for id in ids {
        // Individual conversion failures remain visible on the segment and do not stop recording.
        if let Some(work) = &work {
            queue_conversion(host.clone(), id, work.clone());
        } else {
            let _ = crate::conversion::convert_claimed_segment(host.clone(), id).await;
        }
    }
    Ok(())
}

fn queue_conversion(host: Host, id: i64, work: std::sync::Arc<crate::lifecycle::Operation>) {
    tokio::spawn(async move {
        let _work = work;
        let _ = crate::conversion::convert_claimed_segment(host, id).await;
    });
}
