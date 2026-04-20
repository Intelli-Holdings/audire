// Deepgram Flux v2 streaming ASR client.
//
// Protocol reference: https://developers.deepgram.com/reference/speech-to-text/listen-flux
// EOT tuning: https://developers.deepgram.com/docs/flux/configuration
// CloseStream: https://developers.deepgram.com/docs/flux/close-stream
//
// URL: wss://api.deepgram.com/v2/listen
// Auth: Authorization: Token <KEY>
// Send: binary PCM frames (linear16, 16k, mono)
// Receive: JSON TurnInfo messages with event = StartOfTurn | Update | EagerEndOfTurn | TurnResumed | EndOfTurn
// Stop: send {"type":"CloseStream"}, drain remaining messages for ~2s grace window.

use crate::error::{ParaError, Result};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Default Flux endpoint with tuning params.
/// - model=flux-general-en: English general model
/// - encoding=linear16: 16-bit PCM
/// - sample_rate=16000: 16 kHz mono
/// - eot_threshold=0.7: end-of-turn confidence threshold (default per docs)
/// - eot_timeout_ms=5000: max silence before forced EndOfTurn (default per docs)
const FLUX_URL: &str = "wss://api.deepgram.com/v2/listen\
    ?model=flux-general-en\
    &encoding=linear16\
    &sample_rate=16000\
    &eot_threshold=0.7\
    &eot_timeout_ms=5000";

/// Connect to Deepgram Flux v2 streaming endpoint.
///
/// Returns (sender_channel, receiver_channel) for the ASR pipeline.
/// The sender accepts binary PCM frames and text control messages.
/// The receiver yields server JSON messages (TurnInfo, Connected, Error).
///
/// SECURITY: api_key is used only in the auth header; never logged or returned to UI.
pub async fn connect(api_key: &str) -> Result<(mpsc::Sender<Message>, mpsc::Receiver<Message>)> {
    eprintln!("[asr:dg] connecting to {}", FLUX_URL);

    let request = http::Request::builder()
        .uri(FLUX_URL)
        .header("Authorization", format!("Token {}", api_key))
        .header("Host", "api.deepgram.com")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .map_err(|e| ParaError::Asr(format!("deepgram build request: {}", e)))?;

    let (ws, resp) = connect_async(request)
        .await
        .map_err(|e| {
            eprintln!("[asr:dg] WebSocket connect FAILED: {}", e);
            ParaError::Asr(format!("deepgram flux connect: {}", e))
        })?;

    eprintln!("[asr:dg] WebSocket connected (status: {})", resp.status());

    let (mut ws_write, mut ws_read) = ws.split();

    // Bounded channel: pipeline -> WebSocket (backpressure: 32 frames ≈ 3.2s of audio)
    let (tx, mut tx_rx) = mpsc::channel::<Message>(32);
    // Bounded channel: WebSocket -> pipeline
    let (rx_tx, rx) = mpsc::channel::<Message>(32);

    // Writer task: forward messages from pipeline to WebSocket
    tokio::spawn(async move {
        let mut frames_sent: u64 = 0;
        while let Some(msg) = tx_rx.recv().await {
            let is_binary = matches!(&msg, Message::Binary(_));
            match ws_write.send(msg).await {
                Ok(_) => {
                    if is_binary {
                        frames_sent += 1;
                        if frames_sent == 1 {
                            eprintln!("[asr:dg] first audio frame sent to Deepgram WebSocket");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[asr:dg] writer send error (after {} frames): {}", frames_sent, e);
                    break;
                }
            }
        }
        eprintln!("[asr:dg] writer task ending (sent {} audio frames)", frames_sent);
        let _ = ws_write.close().await;
    });

    // Reader task: forward messages from WebSocket to pipeline
    tokio::spawn(async move {
        let mut msgs_received: u64 = 0;
        loop {
            match ws_read.next().await {
                Some(Ok(msg)) => {
                    msgs_received += 1;
                    match &msg {
                        Message::Text(t) => {
                            if msgs_received <= 3 {
                                eprintln!("[asr:dg] recv msg #{}: {}", msgs_received, &t[..t.len().min(200)]);
                            }
                            if rx_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                        Message::Binary(_) => {
                            if rx_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                        Message::Close(frame) => {
                            eprintln!("[asr:dg] received Close frame: {:?}", frame);
                            break;
                        }
                        _ => {}
                    }
                }
                Some(Err(e)) => {
                    eprintln!("[asr:dg] WebSocket read error (after {} msgs): {}", msgs_received, e);
                    break;
                }
                None => {
                    eprintln!("[asr:dg] WebSocket stream ended (after {} msgs)", msgs_received);
                    break;
                }
            }
        }
        eprintln!("[asr:dg] reader task ending (received {} msgs total)", msgs_received);
    });

    Ok((tx, rx))
}

/// Send the CloseStream control message to gracefully end the Flux session.
/// Reference: https://developers.deepgram.com/docs/flux/close-stream
pub async fn send_close_stream(tx: &mpsc::Sender<Message>) {
    let _ = tx
        .send(Message::Text(r#"{"type":"CloseStream"}"#.into()))
        .await;
}
