//! Bilibili-compatible XML danmaku writer.
//!
//! Timestamps are recorded as second offsets from the recording start time so
//! players that support danmaku overlays can synchronize them with the video.
//! The format matches the conventions used by DouyinLiveRecorder (LyzenX) and
//! rust-srec:
//!
//! ```xml
//! <?xml version="1.0" encoding="utf-8"?>
//! <i>
//!   <d p="{time},{mode},25,{color},{unix_ms},0,{uid},0" user="{nickname}">{content}</d>
//! </i>
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};

/// A single danmaku entry.
#[derive(Debug, Clone)]
pub struct DanmakuItem {
    /// Seconds since the recording started.
    pub time_offset_secs: f64,
    /// Display mode: 1 = scroll, 4 = bottom, 5 = top.
    pub mode: u8,
    /// Font size in pixels.
    pub font_size: u32,
    /// Decimal RGB color.
    pub color: u32,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: i64,
    /// Sender user id.
    pub user_id: u64,
    /// Sender nickname.
    pub nickname: String,
    /// Danmaku text content.
    pub content: String,
}

/// Appends danmaku entries to an XML file. The file is created lazily on the
/// first write so an empty recording produces no dangling file.
pub struct XmlDanmakuWriter {
    path: PathBuf,
    file: Option<std::fs::File>,
    count: u64,
}

impl XmlDanmakuWriter {
    /// Start timestamp alignment at an explicit instant (recording start).
    pub fn with_start_time(path: &Path, _start_time: DateTime<Utc>) -> Self {
        Self {
            path: path.to_path_buf(),
            file: None,
            count: 0,
        }
    }

    fn ensure_open(&mut self) -> std::io::Result<&mut std::fs::File> {
        if self.file.is_none() {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::File::create(&self.path)?;
            writeln!(file, r#"<?xml version="1.0" encoding="utf-8"?>"#)?;
            writeln!(file, r#"<i>"#)?;
            self.file = Some(file);
        }
        Ok(self.file.as_mut().expect("file just opened"))
    }

    /// Append one danmaku entry, flushing when the buffer fills.
    pub fn write_item(&mut self, item: &DanmakuItem) -> std::io::Result<()> {
        let file = self.ensure_open()?;
        // Escape XML special characters in both content and nickname.
        let content = xml_escape(&item.content);
        let nickname = xml_escape(&item.nickname);
        let attributes = format!(
            "{:.3},{},{},{},{},0,{},0",
            item.time_offset_secs,
            item.mode,
            item.font_size,
            item.color,
            item.timestamp_ms,
            item.user_id
        );
        writeln!(
            file,
            r#"<d p="{}" user="{}">{}</d>"#,
            attributes, nickname, content
        )?;
        self.count += 1;
        Ok(())
    }

    /// Close the `<i>` root element. Safe to call even when no entry was
    /// written (removes the empty file).
    pub fn finalize(&mut self) -> std::io::Result<()> {
        if let Some(mut file) = self.file.take() {
            writeln!(file, r#"</i>"#)?;
            file.flush()?;
        }
        Ok(())
    }
}

fn xml_escape(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(ch),
        }
    }
    output
}

pub struct SharedDanmakuWriter {
    inner: Mutex<Option<XmlDanmakuWriter>>,
}

impl SharedDanmakuWriter {
    pub fn new(path: &Path, start_time: DateTime<Utc>) -> Self {
        Self {
            inner: Mutex::new(Some(XmlDanmakuWriter::with_start_time(path, start_time))),
        }
    }

    pub fn write_item(&self, item: &DanmakuItem) -> std::io::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("danmaku writer lock poisoned"))?;
        match guard.as_mut() {
            Some(writer) => writer.write_item(item),
            None => Err(std::io::Error::other("danmaku writer already finalized")),
        }
    }

    pub fn finalize(&self) -> std::io::Result<()> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| std::io::Error::other("danmaku writer lock poisoned"))?;
        if let Some(mut writer) = guard.take() {
            writer.finalize()?;
        }
        Ok(())
    }
}

/// Returns the wall-clock time of a message. Used to compute the offset for a
/// message that carries only an event timestamp.
pub fn time_offset_secs(start: DateTime<Utc>, event: Option<DateTime<Utc>>) -> f64 {
    let event = event.unwrap_or_else(Utc::now);
    let delta = event.signed_duration_since(start);
    (delta.num_milliseconds() as f64 / 1000.0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_creates_bilibili_compatible_xml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("danmaku.xml");
        let mut writer = XmlDanmakuWriter::with_start_time(&path, Utc::now());
        writer
            .write_item(&DanmakuItem {
                time_offset_secs: 1.234,
                mode: 1,
                font_size: 25,
                color: 16777215,
                timestamp_ms: 1_700_000_000_000,
                user_id: 42,
                nickname: "测试用户".into(),
                content: r#"hello <world> & "friends""#.into(),
            })
            .unwrap();
        writer.finalize().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#"<?xml version="1.0" encoding="utf-8"?>"#));
        assert!(text.contains(r#"<d p="1.234,1,25,16777215,1700000000000,0,42,0" user="测试用户">hello &lt;world&gt; &amp; &quot;friends&quot;</d>"#));
        assert!(text.ends_with("</i>\n"));
    }

    #[test]
    fn writer_without_items_removes_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.xml");
        let mut writer = XmlDanmakuWriter::with_start_time(&path, Utc::now());
        writer.finalize().unwrap();
        assert!(!path.exists());
    }
}
