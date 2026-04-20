// Unified ASR event handling for Deepgram Flux v2 and AssemblyAI Universal-3 Pro Streaming.
//
// Deepgram Flux TurnInfo events:
//   type="TurnInfo", event="StartOfTurn|Update|EagerEndOfTurn|TurnResumed|EndOfTurn"
//   transcript field contains the current turn text.
//   Reference: https://developers.deepgram.com/reference/speech-to-text/listen-flux
//
// AssemblyAI Turn messages:
//   type="Turn", end_of_turn=bool, transcript field.
//   Reference: https://www.assemblyai.com/docs/api-reference/streaming-api/universal-3-pro-streaming/universal-3-pro-streaming

use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// Unified ASR event — wraps raw JSON from either provider.
#[derive(Clone, Debug)]
pub enum AsrEvent {
    /// Deepgram Flux v2 TurnInfo message.
    DeepgramFlux(Value),
    /// AssemblyAI Universal-3 Pro Turn/Begin/Termination message.
    AssemblyAi(Value),
    /// Mock provider event for offline testing.
    Mock(Value),
}

impl AsrEvent {
    /// Return a short string describing the event type for diagnostic logging.
    pub fn event_type(&self) -> &'static str {
        match self {
            AsrEvent::DeepgramFlux(_) => "DeepgramFlux",
            AsrEvent::AssemblyAi(_) => "AssemblyAi",
            AsrEvent::Mock(_) => "Mock",
        }
    }

    /// Access the raw JSON for diagnostic logging of unhandled message types.
    pub fn raw_json(&self) -> Option<&Value> {
        match self {
            AsrEvent::DeepgramFlux(v) | AsrEvent::AssemblyAi(v) | AsrEvent::Mock(v) => Some(v),
        }
    }

    /// Whether the provider considers this transcript text formatted.
    /// AssemblyAI exposes `turn_is_formatted`; other providers default to `true`
    /// for finals and `false` for partials.
    pub fn is_formatted(&self) -> bool {
        match self {
            AsrEvent::AssemblyAi(v) => v
                .get("turn_is_formatted")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            AsrEvent::DeepgramFlux(v) => {
                let event = v.get("event").and_then(|x| x.as_str()).unwrap_or("");
                event == "EndOfTurn"
            }
            AsrEvent::Mock(v) => v.get("is_final").and_then(|x| x.as_bool()).unwrap_or(false),
        }
    }

    /// Extract partial (non-final) transcript text.
    /// Returns Some for interim results; None for finals or non-transcript messages.
    pub fn partial_text(&self) -> Option<String> {
        match self {
            AsrEvent::DeepgramFlux(v) => {
                // Flux v2: TurnInfo messages with transcript field.
                let msg_type = v.get("type")?.as_str()?;
                if msg_type != "TurnInfo" {
                    return None;
                }
                let event = v.get("event")?.as_str()?;
                // Partial: Update, StartOfTurn, EagerEndOfTurn, TurnResumed
                // NOT partial: EndOfTurn (that's a final)
                if event == "EndOfTurn" {
                    return None;
                }
                let t = v.get("transcript")?.as_str()?.to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            AsrEvent::AssemblyAi(v) => {
                let ty = v.get("type")?.as_str()?;
                if ty != "Turn" {
                    return None;
                }
                // Partial = not yet formatted (matches AssemblyAI sample: typewriter
                // display while turn_is_formatted is false, commit when true).
                let formatted = v
                    .get("turn_is_formatted")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                if formatted {
                    return None;
                }
                let t = v.get("transcript")?.as_str()?.to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            AsrEvent::Mock(v) => {
                let is_final = v.get("is_final").and_then(|x| x.as_bool()).unwrap_or(false);
                if is_final {
                    return None;
                }
                v.get("text")?.as_str().map(|s| s.to_string())
            }
        }
    }

    /// Extract final (committed) transcript text.
    /// Returns Some only when the provider signals a completed turn/utterance.
    pub fn final_text(&self) -> Option<String> {
        match self {
            AsrEvent::DeepgramFlux(v) => {
                // Flux v2: EndOfTurn event = final transcript for this turn.
                let msg_type = v.get("type")?.as_str()?;
                if msg_type != "TurnInfo" {
                    return None;
                }
                let event = v.get("event")?.as_str()?;
                if event != "EndOfTurn" {
                    return None;
                }
                let t = v.get("transcript")?.as_str()?.to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            AsrEvent::AssemblyAi(v) => {
                let ty = v.get("type")?.as_str()?;
                if ty != "Turn" {
                    return None;
                }
                // Final = formatted turn (matches AssemblyAI sample: only commit
                // when turn_is_formatted is true — the polished version with
                // punctuation and capitalization).
                let formatted = v
                    .get("turn_is_formatted")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                if !formatted {
                    return None;
                }
                let t = v.get("transcript")?.as_str()?.to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            AsrEvent::Mock(v) => {
                let is_final = v.get("is_final").and_then(|x| x.as_bool()).unwrap_or(false);
                if !is_final {
                    return None;
                }
                v.get("text")?.as_str().map(|s| s.to_string())
            }
        }
    }

    /// Check if this is a termination/close confirmation from the server.
    pub fn is_termination(&self) -> bool {
        match self {
            AsrEvent::DeepgramFlux(v) => {
                // Deepgram sends Close frame or Error type on session end.
                v.get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t == "Error")
                    .unwrap_or(false)
            }
            AsrEvent::AssemblyAi(v) => v
                .get("type")
                .and_then(|t| t.as_str())
                .map(|t| t == "Termination")
                .unwrap_or(false),
            AsrEvent::Mock(v) => v
                .get("type")
                .and_then(|t| t.as_str())
                .map(|t| t == "termination")
                .unwrap_or(false),
        }
    }
}

/// Receive the next ASR event from the WebSocket receiver channel.
pub async fn recv_event(rx: &mut mpsc::Receiver<Message>, provider: &str) -> Result<AsrEvent> {
    let msg = rx
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("ws channel closed"))?;

    let text = match msg {
        Message::Text(t) => t.to_string(),
        Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
        Message::Close(_) => return Err(anyhow::anyhow!("ws closed")),
        _ => return Err(anyhow::anyhow!("unexpected ws message type")),
    };

    let v: Value = serde_json::from_str(&text)?;

    Ok(match provider {
        "deepgram" => AsrEvent::DeepgramFlux(v),
        "assemblyai" => AsrEvent::AssemblyAi(v),
        "mock" => AsrEvent::Mock(v),
        _ => AsrEvent::DeepgramFlux(v),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Deepgram Flux v2 TurnInfo parsing tests ----

    #[test]
    fn test_deepgram_flux_start_of_turn() {
        let json = serde_json::json!({
            "type": "TurnInfo",
            "event": "StartOfTurn",
            "turn_index": 0,
            "transcript": "Hello",
            "words": [{"word": "Hello", "confidence": 0.95}],
            "end_of_turn_confidence": 0.1
        });
        let ev = AsrEvent::DeepgramFlux(json);
        assert_eq!(ev.partial_text(), Some("Hello".to_string()));
        assert_eq!(ev.final_text(), None);
        assert!(!ev.is_termination());
    }

    #[test]
    fn test_deepgram_flux_update() {
        let json = serde_json::json!({
            "type": "TurnInfo",
            "event": "Update",
            "turn_index": 0,
            "transcript": "Hello world",
            "words": [],
            "end_of_turn_confidence": 0.3
        });
        let ev = AsrEvent::DeepgramFlux(json);
        assert_eq!(ev.partial_text(), Some("Hello world".to_string()));
        assert_eq!(ev.final_text(), None);
    }

    #[test]
    fn test_deepgram_flux_eager_eot() {
        let json = serde_json::json!({
            "type": "TurnInfo",
            "event": "EagerEndOfTurn",
            "turn_index": 0,
            "transcript": "Hello world how are you",
            "end_of_turn_confidence": 0.75
        });
        let ev = AsrEvent::DeepgramFlux(json);
        // EagerEndOfTurn is still partial (moderate confidence)
        assert_eq!(
            ev.partial_text(),
            Some("Hello world how are you".to_string())
        );
        assert_eq!(ev.final_text(), None);
    }

    #[test]
    fn test_deepgram_flux_turn_resumed() {
        let json = serde_json::json!({
            "type": "TurnInfo",
            "event": "TurnResumed",
            "turn_index": 0,
            "transcript": "Hello world how are you doing"
        });
        let ev = AsrEvent::DeepgramFlux(json);
        assert!(ev.partial_text().is_some());
        assert_eq!(ev.final_text(), None);
    }

    #[test]
    fn test_deepgram_flux_end_of_turn() {
        let json = serde_json::json!({
            "type": "TurnInfo",
            "event": "EndOfTurn",
            "turn_index": 0,
            "transcript": "Hello world how are you doing today",
            "words": [
                {"word": "Hello", "confidence": 0.99},
                {"word": "world", "confidence": 0.98}
            ],
            "end_of_turn_confidence": 0.95
        });
        let ev = AsrEvent::DeepgramFlux(json);
        assert_eq!(ev.partial_text(), None); // EndOfTurn is NOT partial
        assert_eq!(
            ev.final_text(),
            Some("Hello world how are you doing today".to_string())
        );
    }

    #[test]
    fn test_deepgram_connected_ignored() {
        let json = serde_json::json!({
            "type": "Connected",
            "request_id": "abc-123"
        });
        let ev = AsrEvent::DeepgramFlux(json);
        assert_eq!(ev.partial_text(), None);
        assert_eq!(ev.final_text(), None);
    }

    #[test]
    fn test_deepgram_error_is_termination() {
        let json = serde_json::json!({
            "type": "Error",
            "code": "INTERNAL_SERVER_ERROR",
            "description": "something went wrong"
        });
        let ev = AsrEvent::DeepgramFlux(json);
        assert!(ev.is_termination());
    }

    // ---- AssemblyAI Universal-3 Pro parsing tests ----

    #[test]
    fn test_aai_begin_ignored() {
        let json = serde_json::json!({
            "type": "Begin",
            "id": "session-id",
            "expires_at": 1704067200
        });
        let ev = AsrEvent::AssemblyAi(json);
        assert_eq!(ev.partial_text(), None);
        assert_eq!(ev.final_text(), None);
        assert!(!ev.is_termination());
    }

    #[test]
    fn test_aai_turn_partial() {
        // Partial: turn_is_formatted=false (still streaming, typewriter display)
        let json = serde_json::json!({
            "type": "Turn",
            "turn_order": 1,
            "end_of_turn": false,
            "turn_is_formatted": false,
            "transcript": "Hello how are",
            "words": []
        });
        let ev = AsrEvent::AssemblyAi(json);
        assert_eq!(ev.partial_text(), Some("Hello how are".to_string()));
        assert_eq!(ev.final_text(), None);
    }

    #[test]
    fn test_aai_turn_unformatted_eot_is_partial() {
        // end_of_turn=true but NOT yet formatted → still a partial display
        let json = serde_json::json!({
            "type": "Turn",
            "turn_order": 1,
            "end_of_turn": true,
            "turn_is_formatted": false,
            "transcript": "hello how are you today",
            "words": []
        });
        let ev = AsrEvent::AssemblyAi(json);
        assert_eq!(
            ev.partial_text(),
            Some("hello how are you today".to_string())
        );
        assert_eq!(ev.final_text(), None);
    }

    #[test]
    fn test_aai_turn_final() {
        // Final: turn_is_formatted=true (committed, polished text)
        let json = serde_json::json!({
            "type": "Turn",
            "turn_order": 1,
            "end_of_turn": true,
            "turn_is_formatted": true,
            "transcript": "Hello, how are you today?",
            "words": [
                {"text": "Hello", "start": 100, "end": 300, "confidence": 0.99}
            ]
        });
        let ev = AsrEvent::AssemblyAi(json);
        assert_eq!(ev.partial_text(), None);
        assert_eq!(
            ev.final_text(),
            Some("Hello, how are you today?".to_string())
        );
    }

    #[test]
    fn test_aai_termination() {
        let json = serde_json::json!({
            "type": "Termination",
            "audio_duration_seconds": 15,
            "session_duration_seconds": 20
        });
        let ev = AsrEvent::AssemblyAi(json);
        assert!(ev.is_termination());
        assert_eq!(ev.partial_text(), None);
        assert_eq!(ev.final_text(), None);
    }

    // ---- Mock provider tests ----

    #[test]
    fn test_mock_partial() {
        let json = serde_json::json!({"text": "testing", "is_final": false});
        let ev = AsrEvent::Mock(json);
        assert_eq!(ev.partial_text(), Some("testing".to_string()));
        assert_eq!(ev.final_text(), None);
    }

    #[test]
    fn test_mock_final() {
        let json = serde_json::json!({"text": "testing complete", "is_final": true});
        let ev = AsrEvent::Mock(json);
        assert_eq!(ev.partial_text(), None);
        assert_eq!(ev.final_text(), Some("testing complete".to_string()));
    }
}
