// Mock ASR provider for offline UI testing (no network required).
//
// Streams canned partial/final events on a timer so you can verify
// the UI displays transcript updates correctly without real API keys.

use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Simulated transcript turns for testing.
const MOCK_TURNS: &[&str] = &[
    "Welcome everyone to the meeting.",
    "Let's start with the project update.",
    "The new feature is on track for next week.",
    "We need to review the security audit findings.",
    "Action item: schedule follow-up with the team.",
];

/// Start a mock ASR session. Returns channels compatible with the real providers.
/// The mock provider ignores incoming audio and emits canned events on a timer.
pub async fn start_mock() -> (mpsc::Sender<Message>, mpsc::Receiver<Message>) {
    let (tx, _tx_rx) = mpsc::channel::<Message>(32); // sink for audio (ignored)
    let (rx_tx, rx) = mpsc::channel::<Message>(32);

    tokio::spawn(async move {
        // Emit Begin-like event
        let _ = rx_tx
            .send(Message::Text(
                r#"{"type":"begin","text":"","is_final":false}"#.into(),
            ))
            .await;

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        for (i, turn) in MOCK_TURNS.iter().enumerate() {
            let words: Vec<&str> = turn.split_whitespace().collect();

            // Simulate partial updates (word by word)
            let mut partial = String::new();
            for (j, word) in words.iter().enumerate() {
                if j > 0 {
                    partial.push(' ');
                }
                partial.push_str(word);

                let msg = serde_json::json!({
                    "text": partial,
                    "is_final": false,
                    "turn_index": i,
                });
                if let Ok(text) = serde_json::to_string(&msg) {
                    let _ = rx_tx.send(Message::Text(text.into())).await;
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }

            // Final event for this turn
            let msg = serde_json::json!({
                "text": turn,
                "is_final": true,
                "turn_index": i,
            });
            if let Ok(text) = serde_json::to_string(&msg) {
                let _ = rx_tx.send(Message::Text(text.into())).await;
            }

            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        // Termination
        let _ = rx_tx
            .send(Message::Text(
                r#"{"type":"termination","text":"","is_final":false}"#.into(),
            ))
            .await;
    });

    (tx, rx)
}
