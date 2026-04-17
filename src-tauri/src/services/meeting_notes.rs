use std::collections::HashSet;

use serde::Serialize;

use crate::error::{ParaError, Result};
use crate::store::db::{
    LocalStore, NoteRow, SegmentRow, StructuredMeetingNote, StructuredMeetingNoteDraft,
    StructuredNoteCitation, StructuredNoteItemDraft, StructuredNoteSectionDraft,
};

const SECTION_ORDER: [&str; 5] = [
    "key_decisions",
    "action_items",
    "open_questions",
    "risks_blockers",
    "quotes_highlights",
];

#[derive(Debug, Clone, Serialize)]
pub struct MeetingTemplateOption {
    pub kind: String,
    pub label: String,
    pub description: String,
}

pub fn template_options() -> Vec<MeetingTemplateOption> {
    vec![
        template_option(
            "generic",
            "Generic",
            "Balanced summary for most internal or external meetings.",
        ),
        template_option(
            "sales_call",
            "Sales call",
            "Focus on buyer pain, objections, commitments, and next steps.",
        ),
        template_option(
            "one_on_one",
            "1:1",
            "Focus on feedback, support needs, growth topics, and follow-ups.",
        ),
        template_option(
            "standup",
            "Standup",
            "Focus on progress, blockers, and immediate next actions.",
        ),
        template_option(
            "interview",
            "Interview",
            "Focus on candidate signals, open questions, risks, and quotes.",
        ),
        template_option(
            "client_review",
            "Client review",
            "Focus on outcomes, delivery risks, decisions, and requests.",
        ),
    ]
}

pub fn normalize_template_kind(value: Option<&str>) -> String {
    let candidate = value.unwrap_or("generic").trim().to_lowercase();
    if template_options().iter().any(|opt| opt.kind == candidate) {
        candidate
    } else {
        "generic".to_string()
    }
}

pub fn generate_and_store(
    store: &LocalStore,
    meeting_id: &str,
    requested_template: Option<&str>,
) -> Result<StructuredMeetingNote> {
    let meeting = store.get_meeting(meeting_id)?;
    let template_kind =
        normalize_template_kind(requested_template.or(Some(meeting.template_kind.as_str())));
    store.set_meeting_template(meeting_id, &template_kind)?;

    let user_notes = store.get_notes_for_meeting(meeting_id)?;
    let segments = store.segments_for_meeting(meeting_id)?;
    let draft = build_structured_note(store, meeting_id, &template_kind, &user_notes, &segments)?;
    store.replace_structured_meeting_note(meeting_id, &draft)?;
    store
        .get_structured_meeting_note(meeting_id)?
        .ok_or_else(|| ParaError::Db("structured note missing after save".into()))
}

pub fn render_markdown(note: &StructuredMeetingNote) -> String {
    let mut out = String::new();
    out.push_str("# Meeting Notes\n\n");
    out.push_str("## Summary\n\n");
    if note.summary.trim().is_empty() {
        out.push_str("- (no summary generated)\n\n");
    } else {
        out.push_str(&note.summary);
        out.push_str("\n\n");
    }

    for section in &note.sections {
        out.push_str(&format!("## {}\n\n", section_label(&section.kind)));
        if section.items.is_empty() {
            out.push_str("- (none)\n\n");
            continue;
        }
        for item in &section.items {
            out.push_str("- ");
            out.push_str(&item.text);
            if !item.citations.is_empty() {
                let refs = item
                    .citations
                    .iter()
                    .map(|citation| format_timestamp(citation.ts_ms))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(" [{}]", refs));
            }
            out.push('\n');
        }
        out.push('\n');
    }

    out
}

fn build_structured_note(
    store: &LocalStore,
    meeting_id: &str,
    template_kind: &str,
    note_rows: &[NoteRow],
    segments: &[SegmentRow],
) -> Result<StructuredMeetingNoteDraft> {
    let mut sections = SECTION_ORDER
        .iter()
        .map(|kind| StructuredNoteSectionDraft {
            kind: (*kind).to_string(),
            items: Vec::new(),
        })
        .collect::<Vec<_>>();

    let user_lines = extract_user_note_lines(note_rows);
    let mut seen = HashSet::new();

    for line in &user_lines {
        let section_kind = classify_user_line(line);
        let citations = citations_for_text(store, meeting_id, line, 3)?;
        let item = StructuredNoteItemDraft {
            position: next_position(&sections, section_kind),
            author_kind: "user".to_string(),
            text: line.clone(),
            retrieval_confidence: confidence_from_citations(&citations),
            evidence_count: citations.len() as i64,
            citations,
        };
        push_item(&mut sections, section_kind, item, &mut seen);
    }

    for section_kind in SECTION_ORDER {
        let query = template_query(template_kind, section_kind);
        let candidates = store.search_segments_for_query(meeting_id, query, 10)?;
        for segment in candidates
            .into_iter()
            .take(max_items_for_section(section_kind))
        {
            let text = ai_text_for_segment(section_kind, &segment);
            if text.is_empty() {
                continue;
            }
            let citations = vec![StructuredNoteCitation {
                segment_id: segment.id,
                ts_ms: segment.ts_ms,
                source: segment.source.clone(),
                excerpt: excerpt(&segment.text, 180),
            }];
            let item = StructuredNoteItemDraft {
                position: next_position(&sections, section_kind),
                author_kind: "ai".to_string(),
                text,
                retrieval_confidence: segment.confidence.or(Some(0.6)),
                evidence_count: 1,
                citations,
            };
            push_item(&mut sections, section_kind, item, &mut seen);
        }
    }

    let summary = build_summary(template_kind, &user_lines, segments);

    Ok(StructuredMeetingNoteDraft {
        template_kind: template_kind.to_string(),
        ownership_scope: "local_only".to_string(),
        summary,
        sections,
    })
}

fn extract_user_note_lines(note_rows: &[NoteRow]) -> Vec<String> {
    note_rows
        .iter()
        .flat_map(|row| row.text.lines().map(str::trim).collect::<Vec<_>>())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("# Meeting Notes"))
        .filter(|line| !line.starts_with("# Summary"))
        .map(|line| line.trim_start_matches("- ").trim().to_string())
        .collect()
}

fn classify_user_line(line: &str) -> &'static str {
    let lower = line.to_lowercase();
    if contains_any(
        &lower,
        &["decision", "decided", "agreed", "approve", "approved"],
    ) {
        "key_decisions"
    } else if contains_any(
        &lower,
        &[
            "action",
            "todo",
            "follow up",
            "next step",
            "owner",
            "send",
            "share",
            "deliver",
        ],
    ) {
        "action_items"
    } else if contains_any(
        &lower,
        &["question", "unclear", "need to ask", "unknown", "?"],
    ) {
        "open_questions"
    } else if contains_any(
        &lower,
        &["risk", "blocker", "blocked", "issue", "concern", "delay"],
    ) {
        "risks_blockers"
    } else {
        "quotes_highlights"
    }
}

fn build_summary(template_kind: &str, user_lines: &[String], segments: &[SegmentRow]) -> String {
    let mut parts = Vec::new();

    if !user_lines.is_empty() {
        let focus = user_lines
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        parts.push(format!("Human notes led the outline: {}", focus));
    }

    if !segments.is_empty() {
        let transcript_blend = segments
            .iter()
            .take(2)
            .map(|seg| excerpt(&seg.text, 120))
            .collect::<Vec<_>>()
            .join(" ");
        parts.push(format!(
            "{} focus from the transcript: {}",
            template_summary_lead(template_kind),
            transcript_blend
        ));
    }

    if parts.is_empty() {
        "No transcript or notes were available to generate a structured summary.".to_string()
    } else {
        parts.join(" ")
    }
}

fn citations_for_text(
    store: &LocalStore,
    meeting_id: &str,
    text: &str,
    limit: i64,
) -> Result<Vec<StructuredNoteCitation>> {
    let query = keyword_query(text);
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let segments = store.search_segments_for_query(meeting_id, &query, limit)?;
    Ok(segments
        .into_iter()
        .map(|segment| StructuredNoteCitation {
            segment_id: segment.id,
            ts_ms: segment.ts_ms,
            source: segment.source,
            excerpt: excerpt(&segment.text, 180),
        })
        .collect())
}

fn keyword_query(text: &str) -> String {
    let stop_words = [
        "the", "and", "for", "with", "from", "that", "this", "have", "will", "into", "about",
        "your", "they", "them", "then", "were", "what", "when", "where", "which", "should",
    ];

    let keywords = text
        .split(|c: char| !c.is_alphanumeric())
        .map(|word| word.trim().to_lowercase())
        .filter(|word| word.len() > 3)
        .filter(|word| !stop_words.contains(&word.as_str()))
        .take(5)
        .collect::<Vec<_>>();

    keywords.join(" OR ")
}

fn template_query(template_kind: &str, section_kind: &str) -> &'static str {
    match (template_kind, section_kind) {
        ("sales_call", "key_decisions") => "decision OR approved OR budget OR pilot OR pricing",
        ("sales_call", "action_items") => "follow up OR send OR next step OR proposal OR demo",
        ("sales_call", "open_questions") => "question OR concern OR security OR timeline",
        ("sales_call", "risks_blockers") => "risk OR blocker OR objection OR legal OR delay",
        ("sales_call", "quotes_highlights") => "pain OR goal OR outcome OR urgent",
        ("one_on_one", "key_decisions") => "decision OR agreed OR support OR priority",
        ("one_on_one", "action_items") => "follow up OR next step OR owner OR action",
        ("one_on_one", "open_questions") => "question OR unclear OR need",
        ("one_on_one", "risks_blockers") => "blocker OR concern OR risk OR overloaded",
        ("one_on_one", "quotes_highlights") => "highlight OR feedback OR proud",
        ("standup", "key_decisions") => "decision OR ship OR release OR priority",
        ("standup", "action_items") => "today OR next OR action OR owner",
        ("standup", "open_questions") => "question OR waiting OR unclear",
        ("standup", "risks_blockers") => "blocker OR blocked OR dependency OR risk",
        ("standup", "quotes_highlights") => "done OR shipped OR completed",
        ("interview", "key_decisions") => "decision OR recommend OR pass OR hire",
        ("interview", "action_items") => "follow up OR next step OR debrief OR schedule",
        ("interview", "open_questions") => "question OR unclear OR probe OR explore",
        ("interview", "risks_blockers") => "risk OR concern OR gap OR weak",
        ("interview", "quotes_highlights") => "example OR said OR quote OR impact",
        ("client_review", "key_decisions") => "decision OR approve OR roadmap OR priority",
        ("client_review", "action_items") => "follow up OR send OR deliver OR owner",
        ("client_review", "open_questions") => "question OR clarify OR unknown",
        ("client_review", "risks_blockers") => "risk OR blocker OR issue OR delayed",
        ("client_review", "quotes_highlights") => "highlight OR outcome OR request",
        (_, "key_decisions") => "decision OR decided OR agreed OR approved OR priority",
        (_, "action_items") => "action OR next OR follow up OR owner OR send",
        (_, "open_questions") => "question OR unclear OR unknown OR pending",
        (_, "risks_blockers") => "risk OR blocker OR issue OR concern OR delay",
        (_, "quotes_highlights") => "important OR highlight OR notable OR quote",
        _ => "important",
    }
}

fn ai_text_for_segment(section_kind: &str, segment: &SegmentRow) -> String {
    let text = excerpt(&segment.text, 180);
    match section_kind {
        "quotes_highlights" => format!("\"{}\"", text),
        _ => text,
    }
}

fn max_items_for_section(section_kind: &str) -> usize {
    match section_kind {
        "quotes_highlights" => 5,
        _ => 4,
    }
}

fn push_item(
    sections: &mut [StructuredNoteSectionDraft],
    section_kind: &str,
    item: StructuredNoteItemDraft,
    seen: &mut HashSet<String>,
) {
    let fingerprint = format!(
        "{}::{}",
        section_kind,
        item.text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    );
    if !seen.insert(fingerprint) {
        return;
    }

    if let Some(section) = sections
        .iter_mut()
        .find(|section| section.kind == section_kind)
    {
        section.items.push(item);
    }
}

fn next_position(sections: &[StructuredNoteSectionDraft], section_kind: &str) -> i64 {
    sections
        .iter()
        .find(|section| section.kind == section_kind)
        .map(|section| section.items.len() as i64)
        .unwrap_or(0)
}

fn confidence_from_citations(citations: &[StructuredNoteCitation]) -> Option<f64> {
    if citations.is_empty() {
        None
    } else {
        Some((citations.len().min(3) as f64) / 3.0)
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn excerpt(text: &str, max_len: usize) -> String {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let shortened = clean.chars().take(max_len).collect::<String>();
    if clean.chars().count() <= max_len {
        clean
    } else {
        format!("{}...", shortened.trim_end())
    }
}

fn section_label(kind: &str) -> &'static str {
    match kind {
        "key_decisions" => "Key decisions",
        "action_items" => "Action items",
        "open_questions" => "Open questions",
        "risks_blockers" => "Risks / blockers",
        "quotes_highlights" => "Quotes / highlights",
        _ => "Notes",
    }
}

fn format_timestamp(ts_ms: i64) -> String {
    let ts_sec = ts_ms / 1000;
    format!("{:02}:{:02}", (ts_sec / 60) % 60, ts_sec % 60)
}

fn template_option(kind: &str, label: &str, description: &str) -> MeetingTemplateOption {
    MeetingTemplateOption {
        kind: kind.to_string(),
        label: label.to_string(),
        description: description.to_string(),
    }
}

fn template_summary_lead(template_kind: &str) -> &'static str {
    match template_kind {
        "sales_call" => "Commercial",
        "one_on_one" => "Manager / teammate",
        "standup" => "Delivery",
        "interview" => "Candidate",
        "client_review" => "Client",
        _ => "Meeting",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::LocalStore;

    #[test]
    fn generates_structured_notes_with_user_and_ai_items() {
        let store = LocalStore::open_memory().unwrap();
        let meeting_id = store.create_meeting("mock").unwrap();
        let ts = chrono::Utc::now().timestamp_millis();

        store
            .insert_segment(
                &meeting_id,
                "SYS",
                ts,
                "We decided to ship the pilot next Friday and Sam will send the rollout plan.",
                Some(0.91),
            )
            .unwrap();
        store
            .insert_segment(
                &meeting_id,
                "SYS",
                ts + 1000,
                "The blocker is incomplete legal review for the client agreement.",
                Some(0.88),
            )
            .unwrap();
        store
            .insert_note(&meeting_id, "Decision: ship pilot next Friday")
            .unwrap();
        store
            .insert_note(&meeting_id, "Action: Sam sends rollout plan")
            .unwrap();

        let note = generate_and_store(&store, &meeting_id, Some("client_review")).unwrap();
        assert_eq!(note.template_kind, "client_review");
        assert!(note.sections.iter().any(|s| !s.items.is_empty()));
        assert!(note
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .any(|item| item.author_kind == "user"));
        assert!(note
            .sections
            .iter()
            .flat_map(|s| s.items.iter())
            .any(|item| !item.citations.is_empty()));
    }
}
