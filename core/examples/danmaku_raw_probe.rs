//! Raw frame dump for the Douyin webcast WebSocket: prints every frame's
//! type and tries multiple decode paths so protocol issues are visible.
//!
//! Usage: cargo run --example danmaku_raw_probe -- <web_rid> [ttwid]

use std::io::Read;

use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rid = std::env::args()
        .nth(1)
        .expect("usage: danmaku_raw_probe <webcast room id> [ttwid]");
    let ttwid = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "1%7Cdefault".to_string());

    douyin_recorder_core::danmaku::ws::ensure_rustls();

    let url = douyin_recorder_core::danmaku::ws::WsSession::new(&rid, &ttwid, "").websocket_url();
    println!("ws: {url}\n");

    let mut request = url.into_client_request()?;
    request.headers_mut().insert(
        "Origin",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static("https://live.douyin.com"),
    );
    request.headers_mut().insert(
        "User-Agent",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static(
            douyin_recorder_core::danmaku::ws::DOUYIN_UA,
        ),
    );
    request.headers_mut().insert(
        "Cookie",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_str(&format!("ttwid={ttwid}"))?,
    );

    let (mut ws, response) = tokio_tungstenite::connect_async(request).await?;
    println!("connected: status={:?}\n", response.status());

    // Send an immediate heartbeat like the browser client does.
    {
        let hb = douyin_recorder_core::danmaku::proto::PushFrame {
            payload_type: "hb".into(),
            ..Default::default()
        };
        let mut buf = Vec::new();
        hb.encode(&mut buf).unwrap();
        ws.send(WsMessage::Binary(buf.into())).await?;
        println!("→ hb sent");
    }
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(10));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;

    let mut binary_frames = 0u64;
    let mut text_frames = 0u64;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            _ = heartbeat.tick() => {
                let hb = douyin_recorder_core::danmaku::proto::PushFrame {
                    payload_type: "hb".into(),
                    ..Default::default()
                };
                let mut buf = Vec::new();
                hb.encode(&mut buf).unwrap();
                ws.send(WsMessage::Binary(buf.into())).await?;
                println!("→ hb sent");
            }
            frame = ws.next() => {
                match frame {
                    Some(Ok(WsMessage::Binary(data))) => {
                        binary_frames += 1;
                        println!("── binary frame #{binary_frames}, {data} bytes", data = data.len());
                        match douyin_recorder_core::danmaku::proto::PushFrame::decode(&data[..]) {
                            Ok(frame) => {
                                let compress = frame.headers.iter().find(|h| h.key == "compress_type")
                                    .map(|h| h.value.clone()).unwrap_or_else(|| "?".into());
                                println!("   push_frame: seq={} log={} type={} compress={} payload={}B",
                                    frame.seq_id, frame.log_id, frame.payload_type, compress, frame.payload.len());
                                let payload = if compress == "gzip" {
                                    let mut dec = flate2::read::GzDecoder::new(&frame.payload[..]);
                                    let mut out = Vec::new();
                                    dec.read_to_end(&mut out).map(|_| out).unwrap_or_default()
                                } else { frame.payload.clone() };
                                match douyin_recorder_core::danmaku::proto::Response::decode(payload.as_slice()) {
                                    Ok(response) => {
                                        println!("   response: {} messages, need_ack={}", response.messages.len(), response.need_ack);
                                        for message in &response.messages {
                                            println!("     • method={} msg_id={} payload={}B",
                                                message.method, message.msg_id, message.payload.len());
                                            if message.method == "WebcastChatMessage" {
                                                match douyin_recorder_core::danmaku::proto::ChatMessage::decode(message.payload.as_slice()) {
                                                    Ok(chat) => {
                                                        let user = chat.user.map(|u| u.nickname).unwrap_or_default();
                                                        println!("       CHAT: [{user}] {}", chat.content);
                                                    }
                                                    Err(e) => println!("       chat decode error: {e}"),
                                                }
                                            }
                                        }
                                        if response.need_ack {
                                            let ack = douyin_recorder_core::danmaku::proto::PushFrame {
                                                log_id: frame.log_id,
                                                payload_type: "ack".into(),
                                                payload: response.internal_ext.clone().into_bytes(),
                                                ..Default::default()
                                            };
                                            let mut buf = Vec::new();
                                            ack.encode(&mut buf).unwrap();
                                            ws.send(WsMessage::Binary(buf.into())).await?;
                                            println!("   → ack sent");
                                        }
                                    }
                                    Err(e) => println!("   response decode error: {e}"),
                                }
                            }
                            Err(e) => println!("   pushframe decode error: {e}, raw head: {:02x?}", &data[..data.len().min(16)]),
                        }
                    }
                    Some(Ok(WsMessage::Text(text))) => {
                        text_frames += 1;
                        println!("── text frame #{text_frames}: {}", &text[..text.len().min(200)]);
                    }
                    Some(Ok(WsMessage::Ping(p))) => { println!("── ping {}B → pong", p.len()); ws.send(WsMessage::Pong(p)).await?; }
                    Some(Ok(WsMessage::Pong(_))) => println!("── pong"),
                    Some(Ok(WsMessage::Close(info))) => { println!("── close: {info:?}"); break; }
                    Some(Ok(other)) => println!("── other frame: {other:?}"),
                    Some(Err(e)) => { println!("── error: {e}"); break; }
                    None => { println!("── stream ended"); break; }
                }
            }
        }
    }
    println!("\ntotal: {binary_frames} binary, {text_frames} text frames");
    Ok(())
}
