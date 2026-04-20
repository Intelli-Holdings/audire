// AssemblyAI Universal-3 Pro Streaming ASR client.
//
// Protocol reference: https://www.assemblyai.com/docs/api-reference/streaming-api/universal-3-pro-streaming/universal-3-pro-streaming
// ForceEndpoint + Terminate guide: https://www.assemblyai.com/docs/streaming/universal-3-pro
//
// URL: wss://streaming.assemblyai.com/v3/ws?speech_model=u3-rt-pro&sample_rate=16000&encoding=pcm_s16le&format_turns=true&...
// Auth: Authorization: <KEY>
// Send: binary audio chunks between 50ms and 1000ms (we use 100ms = 3200 bytes @ 16kHz mono i16)
// Receive: Begin, Turn (partial if end_of_turn=false, final if end_of_turn=true), Termination
// Stop sequence:
//   1. send {"type":"ForceEndpoint"} to flush current turn
//   2. send {"type":"Terminate"}
//   3. wait up to 2s for Termination message, then close

use crate::error::{ParaError, Result};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// AssemblyAI v3 streaming endpoint with Universal-3 Pro model.
///
/// Key parameters aligned with AssemblyAI reference:
/// - format_turns: enables formatted output (punctuation, capitalization)
/// - end_of_turn_confidence_threshold: sensitivity for end-of-turn detection
/// - min_end_of_turn_silence_when_confident: ms of silence when confident a turn ended
/// - max_turn_silence: ms of silence before forced end of turn
/// - vad_threshold: voice-activity detection sensitivity
const AAI_URL: &str = "wss://streaming.assemblyai.com/v3/ws\
    ?speech_model=u3-rt-pro\
    &sample_rate=16000\
    &encoding=pcm_s16le\
    &format_turns=true\
    &end_of_turn_confidence_threshold=0.4\
    &min_end_of_turn_silence_when_confident=100\
    &max_turn_silence=1000\
    &vad_threshold=0.4";

/// Connect to AssemblyAI Universal-3 Pro Streaming endpoint.
///
/// Returns (sender_channel, receiver_channel) for the ASR pipeline.
///
/// SECURITY: api_key is used only in the auth header; never logged or returned to UI.
pub async fn connect(api_key: &str) -> Result<(mpsc::Sender<Message>, mpsc::Receiver<Message>)> {
    eprintln!("[asr:aai] connecting to {}", AAI_URL);

    let request = http::Request::builder()
        .uri(AAI_URL)
        .header("Authorization", api_key)
        .header("Host", "streaming.assemblyai.com")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .body(())
        .map_err(|e| ParaError::Asr(format!("assemblyai build request: {}", e)))?;

    let (ws, resp) = connect_async(request)
        .await
        .map_err(|e| {
            eprintln!("[asr:aai] WebSocket connect FAILED: {}", e);
            ParaError::Asr(format!("assemblyai connect: {}", e))
        })?;

    eprintln!("[asr:aai] WebSocket connected (status: {})", resp.status());

    let (mut ws_write, mut ws_read) = ws.split();

    // Bounded channels (backpressure: 32 messages)
    let (tx, mut tx_rx) = mpsc::channel::<Message>(32);
    let (rx_tx, rx) = mpsc::channel::<Message>(32);

    // Writer task
    tokio::spawn(async move {
        let mut frames_sent: u64 = 0;
        while let Some(msg) = tx_rx.recv().await {
            let is_binary = matches!(&msg, Message::Binary(_));
            match ws_write.send(msg).await {
                Ok(_) => {
                    if is_binary {
                        frames_sent += 1;
                        if frames_sent == 1 {
                            eprintln!("[asr:aai] first audio frame sent to AssemblyAI WebSocket");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[asr:aai] writer send error (after {} frames): {}", frames_sent, e);
                    break;
                }
            }
        }
        eprintln!("[asr:aai] writer task ending (sent {} audio frames)", frames_sent);
        let _ = ws_write.close().await;
    });

    // Reader task
    tokio::spawn(async move {
        let mut msgs_received: u64 = 0;
        loop {
            match ws_read.next().await {
                Some(Ok(msg)) => {
                    msgs_received += 1;
                    match &msg {
                        Message::Text(t) => {
                            if msgs_received <= 3 {
                                eprintln!("[asr:aai] recv msg #{}: {}", msgs_received, &t[..t.len().min(200)]);
                            }
                            if rx_tx.send(msg).await.is_err() {
                                eprintln!("[asr:aai] reader: downstream channel closed");
                                break;
                            }
                        }
                        Message::Binary(_) => {
                            if rx_tx.send(msg).await.is_err() {
                                eprintln!("[asr:aai] reader: downstream channel closed");
                                break;
                            }
                        }
                        Message::Close(frame) => {
                            eprintln!("[asr:aai] received Close frame: {:?}", frame);
                            break;
                        }
                        other => {
                            eprintln!("[asr:aai] ignoring ws message type: {:?}", other);
                        }
                    }
                }
                Some(Err(e)) => {
                    eprintln!("[asr:aai] WebSocket read error (after {} msgs): {}", msgs_received, e);
                    break;
                }
                None => {
                    eprintln!("[asr:aai] WebSocket stream ended (after {} msgs)", msgs_received);
                    break;
                }
            }
        }
        eprintln!("[asr:aai] reader task ending (received {} msgs total)", msgs_received);
    });

    Ok((tx, rx))
}

/// Send the ForceEndpoint + Terminate sequence to gracefully end the session.
/// Reference: https://www.assemblyai.com/docs/streaming/universal-3-pro ("Forcing a turn endpoint")
pub async fn send_terminate_sequence(tx: &mpsc::Sender<Message>) {
    // Step 1: Force the current turn to end immediately
    let _ = tx
        .send(Message::Text(r#"{"type":"ForceEndpoint"}"#.into()))
        .await;
    // Give AssemblyAI a short window to emit the finalized turn before closing.
    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
    // Step 2: Request session termination
    let _ = tx
        .send(Message::Text(r#"{"type":"Terminate"}"#.into()))
        .await;
}
