//! Manual danmaku probe: validates the full chain against a real Douyin room.
//!
//! Usage:
//!   cargo run --example danmaku_probe -- <live.douyin.com URL> [proxy]
//!
//! Fetches a ttwid session cookie, resolves the room through the webcast enter
//! API, and if the room is live connects the danmaku WebSocket for a few
//! seconds, printing every chat message it receives.

use douyin_recorder_core::danmaku;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .expect("usage: danmaku_probe <live.douyin.com URL> [proxy]");
    let _proxy = std::env::args().nth(2).unwrap_or_default();

    let room_id = url
        .split("live.douyin.com")
        .nth(1)
        .and_then(|rest| rest.trim_start_matches('/').split(['?', '/']).next())
        .filter(|id| !id.is_empty())
        .expect("invalid live.douyin.com URL");

    println!("[1/3] fetching ttwid session from https://live.douyin.com ...");
    let client = reqwest::Client::builder()
        .user_agent(danmaku::ws::DOUYIN_UA)
        .build()?;
    let response = client
        .get("https://live.douyin.com")
        .header("user-agent", danmaku::ws::DOUYIN_UA)
        .send()
        .await?;
    let mut ttwid = String::new();
    for cookie in response.headers().get_all("set-cookie") {
        let cookie = cookie.to_str().unwrap_or("");
        if cookie.starts_with("ttwid=") {
            ttwid = cookie
                .split(';')
                .next()
                .unwrap_or("")
                .trim_start_matches("ttwid=")
                .to_string();
        }
    }
    if ttwid.is_empty() {
        println!("  ! no ttwid returned, continuing with an empty cookie (may fail)");
    } else {
        println!("  ttwid = {ttwid}");
    }

    println!("[2/3] checking room {room_id} live status ...");
    let enter_url = format!(
        "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&enter_from=web_live&cookie_enabled=true&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome&browser_version=120&web_rid={room_id}"
    );
    let info = client
        .get(&enter_url)
        .header("referer", "https://live.douyin.com/")
        .header("cookie", format!("ttwid={ttwid}"))
        .send()
        .await?
        .text()
        .await?;
    let parsed: serde_json::Value = serde_json::from_str(&info)?;
    let status = parsed["data"]["data"]
        .get(0)
        .and_then(|room| room["status"].as_i64());
    let title = parsed["data"]["data"]
        .get(0)
        .and_then(|room| room["title"].as_str())
        .unwrap_or("");
    println!("  title = {title}");
    // The danmaku WebSocket routes by the internal room id (id_str), not the
    // URL's web_rid.
    let webcast_room_id = parsed["data"]["data"]
        .get(0)
        .and_then(|room| room["id_str"].as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| room_id.to_string());
    println!("  status = {status:?} (2 = live), webcast id = {webcast_room_id}");
    if status != Some(2) {
        println!("  ! room is not live, danmaku may be silent");
    }

    println!("[3/3] connecting danmaku websocket (25s window) ...");
    let ws_url = danmaku::ws::WsSession::new(&webcast_room_id, &ttwid, "").websocket_url();
    println!("  ws = {ws_url}");

    let session = danmaku::ws::WsSession::new(&webcast_room_id, &ttwid, "");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let runner = tokio::spawn(async move {
        let _ = danmaku::ws::run_session(session, tx, shutdown_rx).await;
    });

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(25);
    let mut received = 0u64;
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(danmaku::ws::DanmakuEvent::Connected) => println!("  ✓ connected"),
                    Some(danmaku::ws::DanmakuEvent::Chat(item)) => {
                        received += 1;
                        println!("  💬 [{}] {}: {}", item.time_offset_secs, item.nickname, item.content);
                    }
                    Some(danmaku::ws::DanmakuEvent::Reconnecting { attempt, reason }) => {
                        println!("  ⟳ reconnect #{attempt}: {reason}");
                    }
                    Some(danmaku::ws::DanmakuEvent::StreamClosed { tips }) => {
                        println!("  ■ stream closed: {tips:?}");
                        break;
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    let _ = shutdown_tx.send(true);
    runner.abort();
    println!("done: {received} chat messages in the window");
    Ok(())
}
