// AssemblyAI Universal-3 Pro Streaming ASR client.
//
// Protocol reference: https://www.assemblyai.com/docs/api-reference/streaming-api/universal-3-pro-streaming/universal-3-pro-streaming
// ForceEndpoint + Terminate guide: https://www.assemblyai.com/docs/streaming/universal-3-pro
//
// URL: wss://streaming.assemblyai.com/v3/ws?speech_model=u3-rt-pro&sample_rate=16000&encoding=pcm_s16le
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
const AAI_URL: &str = "wss://streaming.assemblyai.com/v3/ws\
    ?speech_model=u3-rt-pro\
    &sample_rate=16000\
    &encoding=pcm_s16le";

/// Connect to AssemblyAI Universal-3 Pro Streaming endpoint.
///
/// Returns (sender_channel, receiver_channel) for the ASR pipeline.
///
/// SECURITY: api_key is used only in the auth header; never logged or returned to UI.
pub async fn connect(
    api_key: &str,
) -> Result<(mpsc::Sender<Message>, mpsc::Receiver<Message>)> {
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

    let (ws, _resp) = connect_async(request)
        .await
        .map_err(|e| ParaError::Asr(format!("assemblyai connect: {}", e)))?;

    let (mut ws_write, mut ws_read) = ws.split();

    // Bounded channels (backpressure: 32 messages)
    let (tx, mut tx_rx) = mpsc::channel::<Message>(32);
    let (rx_tx, rx) = mpsc::channel::<Message>(32);

    // Writer task
    tokio::spawn(async move {
        while let Some(msg) = tx_rx.recv().await {
            if ws_write.send(msg).await.is_err() {
                break;
            }
        }
        let _ = ws_write.close().await;
    });

    // Reader task
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_read.next().await {
            match &msg {
                Message::Text(_) | Message::Binary(_) => {
                    if rx_tx.send(msg).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
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
    // Step 2: Request session termination
    let _ = tx
        .send(Message::Text(r#"{"type":"Terminate"}"#.into()))
        .await;
}
