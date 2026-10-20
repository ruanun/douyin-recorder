//! Convert recorded danmaku XML into ASS subtitles.
//!
//! The XML format is the Bilibili-compatible one we write while recording:
//!
//! ```xml
//! <?xml version="1.0" encoding="utf-8"?>
//! <i>
//! <d p="12.345,1,25,16777215,1760341234567,0,42,0" user="nickname">text</d>
//! </i>
//! ```
//!
//! `p` fields: time offset (s), mode (1 scroll, 4 bottom, 5 top), font size,
//! decimal RGB color, unix ms, pool, sender id, row id.
//!
//! The generated ASS uses `\move` for scrolling comments and `\pos` for fixed
//! ones, with per-comment color overrides. Lane allocation mirrors the proven
//! algorithm from danmaku2ass (m13253/danmaku2ass) and danmu2ass
//! (gwy15/danmu2ass): a lane accepts a new comment once the previous one has
//! entered far enough that they can never collide.

use std::fmt::Write as _;
use std::path::Path;

/// One parsed danmaku comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    /// Seconds since the recording start.
    pub time: f64,
    /// 1 = scroll right-to-left, 4 = bottom, 5 = top.
    pub mode: u8,
    /// Font size as stored by Douyin (25 is its default).
    pub size: u32,
    /// Decimal RGB color.
    pub color: u32,
    pub nickname: String,
    pub text: String,
}

/// Subtitle rendering options. Coordinates are in ASS logical pixels; libass
/// scales them to the actual video size.
#[derive(Debug, Clone)]
pub struct AssConfig {
    pub width: u32,
    pub height: u32,
    pub font: String,
    pub font_size: u32,
    /// How long a scrolling comment takes to cross the screen.
    pub scroll_duration_secs: f64,
    /// How long a top/bottom comment stays visible.
    pub still_duration_secs: f64,
    /// Horizontal gap kept between consecutive comments in a lane (px).
    pub gap: f64,
    /// Pixels reserved at the bottom (players' controls).
    pub bottom_reserved: u32,
    /// Outline width; 0 renders bare text.
    pub outline: f64,
}

impl Default for AssConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            font: "Microsoft YaHei".to_string(),
            font_size: 48,
            scroll_duration_secs: 8.0,
            still_duration_secs: 4.0,
            gap: 4.0,
            bottom_reserved: 0,
            outline: 1.5,
        }
    }
}

/// Parse a Bilibili-compatible danmaku XML document. Unknown elements are
/// ignored so files from other recorders still work.
pub fn parse_xml(content: &str) -> Result<Vec<Comment>, String> {
    let mut comments = Vec::new();
    let mut reader = quick_xml::Reader::from_str(content);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(quick_xml::events::Event::Start(element)) => {
                if element.name().as_ref() != b"d" {
                    buffer.clear();
                    continue;
                }
                let (params, nickname) = read_attributes(&element, &reader)?;
                // Text lives between <d> and </d>.
                let mut text = String::new();
                loop {
                    match reader.read_event_into(&mut Vec::new()) {
                        Ok(quick_xml::events::Event::Text(value)) => {
                            text.push_str(
                                &value
                                    .unescape()
                                    .map_err(|error| format!("解析弹幕内容失败: {error}"))?,
                            );
                        }
                        Ok(quick_xml::events::Event::CData(value)) => {
                            text.push_str(&String::from_utf8_lossy(&value));
                        }
                        Ok(quick_xml::events::Event::End(_))
                        | Ok(quick_xml::events::Event::Eof) => break,
                        Ok(_) => {}
                        Err(error) => return Err(format!("解析弹幕 XML 失败: {error}")),
                    }
                }
                if let Some(comment) = build_comment(&params, &nickname, &text) {
                    comments.push(comment);
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("解析弹幕 XML 失败: {error}")),
        }
        buffer.clear();
    }
    comments.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(comments)
}

fn read_attributes(
    element: &quick_xml::events::BytesStart<'_>,
    reader: &quick_xml::Reader<&[u8]>,
) -> Result<(String, String), String> {
    let mut params = String::new();
    let mut nickname = String::new();
    for attribute in element.attributes().flatten() {
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| format!("解析弹幕属性失败: {error}"))?
            .into_owned();
        match attribute.key.as_ref() {
            b"p" => params = value,
            b"user" => nickname = value,
            _ => {}
        }
    }
    Ok((params, nickname))
}

fn build_comment(params: &str, nickname: &str, text: &str) -> Option<Comment> {
    if text.trim().is_empty() {
        return None;
    }
    let mut fields = params.split(',');
    let time = fields
        .next()
        .and_then(|value| value.trim().parse::<f64>().ok())?;
    let mode = fields
        .next()
        .and_then(|value| value.trim().parse::<u8>().ok())
        .unwrap_or(1);
    let size = fields
        .next()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(25);
    let color = fields
        .next()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0xFFFFFF);
    Some(Comment {
        time: time.max(0.0),
        mode,
        size,
        color,
        nickname: nickname.to_string(),
        text: text.to_string(),
    })
}

/// Convert danmaku XML text into ASS text, returning the subtitle along with
/// the number of comments that were rendered.
pub fn convert_text(content: &str, config: &AssConfig) -> Result<(String, usize), String> {
    let comments = parse_xml(content)?;
    let count = comments.len();
    Ok((render(&comments, config), count))
}

/// Convert a danmaku XML file into ASS text.
pub fn convert_file(xml_path: &Path, config: &AssConfig) -> Result<String, String> {
    let content =
        std::fs::read_to_string(xml_path).map_err(|error| format!("读取弹幕文件失败: {error}"))?;
    let comments = parse_xml(&content)?;
    Ok(render(&comments, config))
}

/// Render parsed comments as an ASS document.
pub fn render(comments: &[Comment], config: &AssConfig) -> String {
    let mut output = String::with_capacity(comments.len() * 160 + 2048);
    write_header(&mut output, config);

    let lane_height = (config.font_size as f64 * 1.25).max(1.0);
    let lane_count = ((config.height.saturating_sub(config.bottom_reserved)) as f64 / lane_height)
        .floor()
        .max(1.0) as usize;

    // Lanes from top to bottom for scrolling and top-fixed comments.
    let mut scroll_lanes: Vec<Option<Lane>> = (0..lane_count).map(|_| None).collect();
    // Lanes counted from the bottom for bottom-fixed comments.
    let mut bottom_lanes: Vec<Option<Lane>> = (0..lane_count).map(|_| None).collect();

    for comment in comments {
        let length = estimate_length(&comment.text, config.font_size);
        let duration = match comment.mode {
            4 | 5 => config.still_duration_secs,
            _ => config.scroll_duration_secs,
        };
        let start = comment.time;
        let end = start + duration;
        let color = ass_color(comment.color);
        // Bilibili uses 18 (small), 25 (medium) and 36 (large). Scale the base
        // font size proportionally via an inline override.
        let size_tag = if comment.size == 0 || comment.size == 25 {
            String::new()
        } else {
            let scaled = (config.font_size as f64 * comment.size as f64 / 25.0).round() as u32;
            format!("\\fs{scaled}")
        };
        let text = escape_text(&comment.text);
        let name = escape_name(&comment.nickname);
        let is_still = matches!(comment.mode, 4 | 5);

        let (lane_index, lane_top) = if comment.mode == 4 {
            let usable_bottom = (config.height.saturating_sub(config.bottom_reserved)) as f64;
            match pick_lane(&mut bottom_lanes, comment, config, length, is_still) {
                Some(index) => (index, usable_bottom - (index as f64 + 1.0) * lane_height),
                None => continue,
            }
        } else {
            match pick_lane(&mut scroll_lanes, comment, config, length, is_still) {
                Some(index) => (index, index as f64 * lane_height),
                None => continue,
            }
        };
        let lanes = if comment.mode == 4 {
            &mut bottom_lanes
        } else {
            &mut scroll_lanes
        };
        lanes[lane_index] = Some(Lane {
            start,
            length,
            duration,
        });

        let y = lane_top + lane_height / 2.0;
        let effect = match comment.mode {
            4 => format!("\\an2\\pos({:.0},{:.0})", config.width as f64 / 2.0, y),
            5 => format!("\\an8\\pos({:.0},{:.0})", config.width as f64 / 2.0, y),
            _ => format!(
                "\\an7\\move({:.0},{:.0},{:.0},{:.0})",
                config.width as f64,
                y,
                -length.ceil(),
                y
            ),
        };
        let _ = writeln!(
            output,
            "Dialogue: 0,{},{},Danmaku,{},0,0,0,,{{{}{}\\c&H{}&}}{}",
            ass_time(start),
            ass_time(end),
            name,
            effect,
            size_tag,
            color,
            text
        );
    }
    output
}

struct Lane {
    start: f64,
    length: f64,
    duration: f64,
}

fn pick_lane(
    lanes: &mut [Option<Lane>],
    comment: &Comment,
    config: &AssConfig,
    length: f64,
    is_still: bool,
) -> Option<usize> {
    // Only a lane that is already free is acceptable: drawing on a busy lane
    // would overlap or show the comment earlier than its recorded timestamp.
    // Like danmaku2ass, overflowing comments are dropped to keep the video
    // readable during bursts.
    for (index, lane) in lanes.iter().enumerate() {
        let free = match lane {
            None => true,
            Some(lane) if is_still => comment.time >= lane.start + lane.duration,
            Some(lane) => {
                let width = config.width as f64;
                let gap = config.gap;
                // Time until the previous comment's tail clears the entry point.
                let enter = (lane.length + gap) * lane.duration / (lane.length + width);
                // Time needed so a longer (faster) comment cannot catch up.
                let speed = (width + length) / config.scroll_duration_secs;
                let catch_up = lane.duration - (width - gap).max(0.0) / speed;
                let required = enter.max(catch_up);
                comment.time >= lane.start + required
            }
        };
        if free {
            return Some(index);
        }
    }
    None
}

fn write_header(output: &mut String, config: &AssConfig) {
    let _ = write!(
        output,
        "[Script Info]
Title: Douyin Danmaku
ScriptType: v4.00+
WrapStyle: 2
ScaledBorderAndShadow: yes
PlayResX: {width}
PlayResY: {height}
Collisions: None

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Danmaku,{font},{size},&H00FFFFFF,&H00FFFFFF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,{outline},0.5,7,0,0,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
",
        width = config.width,
        height = config.height,
        font = config.font,
        size = config.font_size,
        outline = config.outline,
    );
}

/// Rough pixel width: CJK characters are full-width, Latin roughly half.
fn estimate_length(text: &str, font_size: u32) -> f64 {
    let size = font_size as f64;
    text.chars()
        .map(|character| {
            let scalar = character as u32;
            let wide = matches!(scalar,
                0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
                | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
                | 0x20000..=0x3FFFD);
            if wide {
                size
            } else if character == ' ' {
                size * 0.3
            } else {
                size * 0.55
            }
        })
        .sum()
}

/// ASS timestamps use `H:MM:SS.cc` (centiseconds).
fn ass_time(seconds: f64) -> String {
    let total = (seconds.max(0.0) * 100.0).round() as u64;
    let hours = total / 360_000;
    let minutes = (total % 360_000) / 6_000;
    let secs = (total % 6_000) / 100;
    let centis = total % 100;
    format!("{hours}:{minutes:02}:{secs:02}.{centis:02}")
}

/// ASS colors are `&HBBGGRR&` (reversed byte order, `&H00BBGGRR` with alpha).
fn ass_color(rgb: u32) -> String {
    let red = (rgb >> 16) & 0xFF;
    let green = (rgb >> 8) & 0xFF;
    let blue = rgb & 0xFF;
    format!("{blue:02X}{green:02X}{red:02X}")
}

/// Escape text so libass does not interpret it as override tags. Following
/// danmaku2ass: a backslash is followed by a zero-width space, braces are
/// escaped, and newlines become `\N`.
fn escape_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 8);
    let mut line_start = true;
    for character in text.chars() {
        match character {
            '\\' => result.push_str("\\\u{200b}"),
            '{' => result.push_str("\\{"),
            '}' => result.push_str("\\}"),
            '\n' => {
                result.push_str("\\N");
                line_start = true;
                continue;
            }
            ' ' if line_start => result.push('\u{00a0}'),
            other => result.push(other),
        }
        line_start = false;
    }
    result
}

fn escape_name(name: &str) -> String {
    name.replace(',', "，").replace('\n', " ")
}

/// Where to write the subtitle file for a given recording file.
pub fn ass_path_for(media_path: &Path) -> std::path::PathBuf {
    media_path.with_extension("ass")
}

/// Convert `xml_path` into `<same name>.ass` and return the new path.
pub fn export_ass_file(xml_path: &Path) -> Result<std::path::PathBuf, String> {
    let content = convert_file(xml_path, &AssConfig::default())?;
    let target = ass_path_for(xml_path);
    std::fs::write(&target, content).map_err(|error| format!("写入字幕文件失败: {error}"))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<i>
<d p="1.500,1,25,16777215,1760341234567,0,42,0" user="观众甲">哈哈哈</d>
<d p="2.000,5,25,16711680,1760341234999,0,43,0" user="观众乙">顶部 {tag}</d>
<d p="3.250,4,25,255,1760341235555,0,44,0" user="观众丙">底部弹幕</d>
</i>"#;

    #[test]
    fn parses_comments_with_modes_colors_and_entities() {
        let comments = parse_xml(SAMPLE).unwrap();
        assert_eq!(comments.len(), 3);
        assert_eq!(comments[0].time, 1.5);
        assert_eq!(comments[0].text, "哈哈哈");
        assert_eq!(comments[0].nickname, "观众甲");
        assert_eq!(comments[1].mode, 5);
        assert_eq!(comments[1].color, 0xFF0000);
        assert_eq!(comments[2].mode, 4);
        assert_eq!(comments[2].color, 0x0000FF);
    }

    #[test]
    fn renders_ass_with_move_and_pos_events() {
        let comments = parse_xml(SAMPLE).unwrap();
        let ass = render(&comments, &AssConfig::default());
        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("PlayResX: 1920"));
        // Scroll comment: starts at the right edge and ends off-screen left.
        assert!(ass.contains("0:00:01.50,0:00:09.50,Danmaku,"));
        assert!(ass.contains("\\move(1920,"));
        // Top comment keeps its position and its red color (ASS is BBGGRR).
        assert!(ass.contains("\\an8\\pos(960,"));
        assert!(ass.contains("\\c&H0000FF&"));
        // Bottom comment is anchored at the bottom edge.
        assert!(ass.contains("\\an2\\pos(960,"));
        // Braces in the text are escaped so libass does not parse them as tags.
        assert!(ass.contains("\\{tag\\}"));
    }

    #[test]
    fn non_default_sizes_emit_font_overrides() {
        let comments = vec![
            Comment {
                time: 0.0,
                mode: 1,
                size: 18,
                color: 0xFFFFFF,
                nickname: String::new(),
                text: "small".into(),
            },
            Comment {
                time: 0.0,
                mode: 1,
                size: 36,
                color: 0xFFFFFF,
                nickname: String::new(),
                text: "large".into(),
            },
        ];
        let ass = render(&comments, &AssConfig::default());
        assert!(ass.contains("\\fs35")); // 48 * 18 / 25 = 34.56 -> 35
        assert!(ass.contains("\\fs69")); // 48 * 36 / 25 = 69.12 -> 69
    }

    #[test]
    fn oversized_bursts_are_dropped_instead_of_overlapping() {
        let config = AssConfig {
            scroll_duration_secs: 8.0,
            ..AssConfig::default()
        };
        // Fill every lane at t=0 with long comments, then add more at t=0.1:
        // the extra ones cannot be placed collision-free and must be dropped.
        let mut comments = Vec::new();
        let lane_count = (config.height as f64 / (config.font_size as f64 * 1.25)).floor() as usize;
        for _ in 0..lane_count {
            comments.push(Comment {
                time: 0.0,
                mode: 1,
                size: 25,
                color: 0xFFFFFF,
                nickname: String::new(),
                text: "很长的弹幕内容占满整行".into(),
            });
        }
        comments.push(Comment {
            time: 0.1,
            mode: 1,
            size: 25,
            color: 0xFFFFFF,
            nickname: String::new(),
            text: "爆满时新增".into(),
        });
        let ass = render(&comments, &config);
        let drawn = ass
            .lines()
            .filter(|line| line.starts_with("Dialogue"))
            .count();
        assert_eq!(drawn, lane_count);
    }

    #[test]
    fn timestamps_format_with_centiseconds() {
        assert_eq!(ass_time(0.0), "0:00:00.00");
        assert_eq!(ass_time(65.239), "0:01:05.24");
        assert_eq!(ass_time(3661.5), "1:01:01.50");
    }

    #[test]
    fn colors_convert_to_ass_byte_order() {
        assert_eq!(ass_color(0xFFFFFF), "FFFFFF");
        // ASS stores colors as BBGGRR.
        assert_eq!(ass_color(0xFF0000), "0000FF"); // red
        assert_eq!(ass_color(0x00FF00), "00FF00"); // green
        assert_eq!(ass_color(0x0000FF), "FF0000"); // blue
    }

    #[test]
    fn lanes_are_reused_only_after_collision_free_delay() {
        let config = AssConfig::default();
        let comments = vec![
            Comment {
                time: 0.0,
                mode: 1,
                size: 25,
                color: 0xFFFFFF,
                nickname: String::new(),
                text: "short".into(),
            },
            // Same instant: must go to a different lane.
            Comment {
                time: 0.0,
                mode: 1,
                size: 25,
                color: 0xFFFFFF,
                nickname: String::new(),
                text: "second".into(),
            },
        ];
        let ass = render(&comments, &config);
        let moves: Vec<&str> = ass.match_indices("\\move(1920,").map(|(_, m)| m).collect();
        assert_eq!(moves.len(), 2);
        // Different lane => different y coordinates.
        let ys: Vec<String> = ass
            .lines()
            .filter(|line| line.contains("\\move(1920,"))
            .map(|line| {
                line.split("\\move(1920,")
                    .nth(1)
                    .unwrap()
                    .split(',')
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_ne!(ys[0], ys[1]);
    }

    #[test]
    fn escaped_backslashes_do_not_create_tags() {
        assert_eq!(escape_text("a\\b"), "a\\\u{200b}b");
    }
}
