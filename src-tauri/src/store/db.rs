use crate::error::{ParaError, Result};

use chrono::Utc;
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Encrypted local store (SQLCipher) for transcripts and notes.
///
/// Privacy guarantees:
/// - No audio is ever written to disk.
/// - Only text transcripts + user notes are persisted.
/// - DB file is encrypted at rest via SQLCipher.
/// - DB key is from OS keyring (BYOK); never returned to WebView.
#[derive(Clone)]
pub struct LocalStore {
    inner: Arc<Mutex<Connection>>,
}

impl LocalStore {
    /// Open (or create) the default DB in the app data directory.
    /// If `db_key` is Some, applies SQLCipher PRAGMA key before any operations.
    pub fn open_default(db_key: Option<&str>) -> Result<Self> {
        let proj = ProjectDirs::from("com", "audire", "Audire")
            .ok_or_else(|| ParaError::Db("failed to resolve project dirs".into()))?;
        let dir = proj.data_dir();
        std::fs::create_dir_all(dir).map_err(|e| ParaError::Db(e.to_string()))?;
        let db_path = dir.join("audire.db");

        let conn = Connection::open(&db_path).map_err(|e| ParaError::Db(e.to_string()))?;

        // Apply SQLCipher encryption key before any other operations.
        // IMPORTANT: never hardcode keys; never return keys to frontend.
        if let Some(key) = db_key {
            conn.pragma_update(None, "key", key)
                .map_err(|e| ParaError::Db(format!("sqlcipher key: {}", e)))?;
        }

        migrate(&conn)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory DB for testing.
    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| ParaError::Db(e.to_string()))?;
        migrate(&conn)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn create_meeting(&self, provider: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO meetings(id, provider, started_at) VALUES (?1, ?2, ?3)",
            params![id, provider, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(id)
    }

    pub fn end_meeting(&self, meeting_id: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET ended_at=?2 WHERE id=?1",
            params![meeting_id, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn update_meeting_title(&self, meeting_id: &str, title: &str) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET title=?2 WHERE id=?1",
            params![meeting_id, title],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    /// Insert a transcript segment.
    /// * `source` - "SYS" (system audio) or "MIC" (microphone)
    /// * `ts_ms` - timestamp in milliseconds since epoch
    /// * `confidence` - optional ASR confidence score
    pub fn insert_segment(
        &self,
        meeting_id: &str,
        source: &str,
        ts_ms: i64,
        text: &str,
        confidence: Option<f64>,
    ) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO segments(meeting_id, source, ts_ms, text, confidence) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![meeting_id, source, ts_ms, text, confidence],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn insert_note(&self, meeting_id: &str, text: &str) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO notes(meeting_id, ts_ms, text) VALUES (?1, ?2, ?3)",
            params![meeting_id, now, text],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn segments_for_meeting(&self, meeting_id: &str) -> Result<Vec<SegmentRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, source, ts_ms, text, confidence FROM segments \
                 WHERE meeting_id=?1 ORDER BY id ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![meeting_id], |row| {
                Ok(SegmentRow {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    ts_ms: row.get(2)?,
                    text: row.get(3)?,
                    confidence: row.get(4)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn search_segments_for_query(
        &self,
        meeting_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<SegmentRow>> {
        let conn = self.inner.lock().unwrap();

        let fts_sql = r#"
            SELECT s.id, s.source, s.ts_ms, s.text, s.confidence
            FROM segments_fts f
            JOIN segments s ON s.id = f.rowid
            WHERE s.meeting_id = ?1
              AND segments_fts MATCH ?2
            ORDER BY bm25(segments_fts)
            LIMIT ?3
        "#;

        let result = match conn.prepare(fts_sql) {
            Ok(mut stmt) => {
                let rows = stmt
                    .query_map(params![meeting_id, query, limit], |row| {
                        Ok(SegmentRow {
                            id: row.get(0)?,
                            source: row.get(1)?,
                            ts_ms: row.get(2)?,
                            text: row.get(3)?,
                            confidence: row.get(4)?,
                        })
                    })
                    .map_err(|e| ParaError::Db(e.to_string()))?;

                let mut out = Vec::new();
                for r in rows {
                    out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
                }
                Ok(out)
            }
            Err(_) => {
                let like_query = query
                    .replace(" OR ", " ")
                    .replace(" AND ", " ")
                    .replace('\"', "");
                let mut stmt = conn
                    .prepare(
                        "SELECT id, source, ts_ms, text, confidence
                         FROM segments
                         WHERE meeting_id=?1
                           AND text LIKE '%' || ?2 || '%'
                         ORDER BY id ASC
                         LIMIT ?3",
                    )
                    .map_err(|e| ParaError::Db(e.to_string()))?;

                let rows = stmt
                    .query_map(params![meeting_id, like_query.trim(), limit], |row| {
                        Ok(SegmentRow {
                            id: row.get(0)?,
                            source: row.get(1)?,
                            ts_ms: row.get(2)?,
                            text: row.get(3)?,
                            confidence: row.get(4)?,
                        })
                    })
                    .map_err(|e| ParaError::Db(e.to_string()))?;

                let mut out = Vec::new();
                for r in rows {
                    out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
                }
                Ok(out)
            }
        };

        result
    }

    pub fn notes_for_meeting(&self, meeting_id: &str) -> Result<Vec<String>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT text FROM notes WHERE meeting_id=?1 ORDER BY id ASC")
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![meeting_id], |row| row.get::<_, String>(0))
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn top_segments_for_query(
        &self,
        meeting_id: &str,
        query: &str,
        limit: i64,
    ) -> Result<Vec<String>> {
        let conn = self.inner.lock().unwrap();

        let fts_sql = r#"
            SELECT s.text
            FROM segments_fts f
            JOIN segments s ON s.id = f.rowid
            WHERE s.meeting_id = ?1
              AND segments_fts MATCH ?2
            ORDER BY bm25(segments_fts)
            LIMIT ?3
        "#;

        let result = match conn.prepare(fts_sql) {
            Ok(mut stmt) => {
                let rows = stmt
                    .query_map(params![meeting_id, query, limit], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(|e| ParaError::Db(e.to_string()))?;

                let mut out = Vec::new();
                for r in rows {
                    out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
                }
                Ok(out)
            }
            Err(_) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT text FROM segments
                         WHERE meeting_id=?1
                           AND text LIKE '%' || ?2 || '%'
                         ORDER BY id ASC
                         LIMIT ?3",
                    )
                    .map_err(|e| ParaError::Db(e.to_string()))?;

                let rows = stmt
                    .query_map(params![meeting_id, query, limit], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(|e| ParaError::Db(e.to_string()))?;

                let mut out = Vec::new();
                for r in rows {
                    out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
                }
                Ok(out)
            }
        };

        result
    }

    /// Generate structured Markdown notes for a meeting.
    /// Called after stop_capture to create the meeting notes.
    ///
    /// Format:
    /// # Meeting Notes
    /// ## Key points
    /// ## Transcript
    pub fn generate_meeting_notes(&self, meeting_id: &str) -> Result<()> {
        let segments = self.segments_for_meeting(meeting_id)?;
        let notes = self.notes_for_meeting(meeting_id)?;

        let mut md = String::new();
        md.push_str("# Meeting Notes\n\n");

        md.push_str("## Key points\n\n");
        if notes.is_empty() {
            md.push_str("- (no notes taken during meeting)\n");
        } else {
            for n in &notes {
                md.push_str("- ");
                md.push_str(n);
                if !n.ends_with('\n') {
                    md.push('\n');
                }
            }
        }

        md.push_str("\n## Transcript\n\n");
        if segments.is_empty() {
            md.push_str("- (no transcript captured)\n");
        } else {
            for seg in &segments {
                md.push_str(&format!("- {}\n", format_segment(seg)));
            }
        }

        self.insert_note(meeting_id, &md)?;
        Ok(())
    }

    pub fn export_meeting_markdown(
        &self,
        meeting_id: &str,
        app_data_dir: PathBuf,
    ) -> Result<PathBuf> {
        let segments = self.segments_for_meeting(meeting_id)?;
        let notes = self.notes_for_meeting(meeting_id)?;
        let structured = self.get_structured_meeting_note(meeting_id)?;

        let export_dir = app_data_dir.join("exports");
        std::fs::create_dir_all(&export_dir).map_err(|e| ParaError::Db(e.to_string()))?;
        let out_path = export_dir.join(format!("meeting-{}.md", meeting_id));

        let mut md = String::new();
        md.push_str("# Audire Export\n\n");
        md.push_str(&format!("Meeting: `{}`\n\n", meeting_id));

        if let Some(structured) = structured {
            md.push_str("## Structured Notes\n\n");
            if structured.summary.trim().is_empty() {
                md.push_str("- (no structured summary)\n");
            } else {
                md.push_str(&structured.summary);
                md.push_str("\n\n");
            }
            for section in structured.sections {
                md.push_str(&format!("### {}\n\n", section.label));
                if section.items.is_empty() {
                    md.push_str("- (none)\n\n");
                    continue;
                }
                for item in section.items {
                    md.push_str("- ");
                    md.push_str(&item.text);
                    if !item.citations.is_empty() {
                        let refs = item
                            .citations
                            .iter()
                            .map(|c| {
                                let ts_sec = c.ts_ms / 1000;
                                format!("{:02}:{:02}", (ts_sec / 60) % 60, ts_sec % 60)
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        md.push_str(&format!(" [{}]", refs));
                    }
                    md.push('\n');
                }
                md.push('\n');
            }
            md.push_str("## User Notes\n\n");
        } else {
            md.push_str("## Notes\n\n");
        }
        if notes.is_empty() {
            md.push_str("- (no notes)\n");
        } else {
            for n in &notes {
                md.push_str("- ");
                md.push_str(n);
                if !n.ends_with('\n') {
                    md.push('\n');
                }
            }
        }

        md.push_str("\n## Transcript\n\n");
        if segments.is_empty() {
            md.push_str("- (no transcript)\n");
        } else {
            for seg in &segments {
                md.push_str(&format!("- {}\n", format_segment(seg)));
            }
        }

        std::fs::write(&out_path, md).map_err(|e| ParaError::Db(e.to_string()))?;

        let conn = self.inner.lock().unwrap();
        let now = Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT INTO export_cache(meeting_id, format, path, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![meeting_id, "md", out_path.display().to_string(), now],
        );

        Ok(out_path)
    }

    pub fn get_meeting(&self, meeting_id: &str) -> Result<MeetingDetailRow> {
        let conn = self.inner.lock().unwrap();
        conn.query_row(
            "SELECT m.id,
                    m.provider,
                    m.started_at,
                    m.ended_at,
                    m.title,
                    m.template_kind,
                    m.ownership_scope,
                    m.folder_id,
                    f.name,
                    (
                        SELECT COUNT(*)
                        FROM notes n
                        WHERE n.meeting_id = m.id
                    ) AS note_count,
                    EXISTS(
                        SELECT 1
                        FROM meeting_structured_notes msn
                        WHERE msn.meeting_id = m.id
                    ) AS has_structured_notes
             FROM meetings m
             LEFT JOIN folders f ON f.id = m.folder_id
             WHERE m.id = ?1",
            params![meeting_id],
            |row| {
                Ok(MeetingDetailRow {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    started_at: row.get(2)?,
                    ended_at: row.get(3)?,
                    title: row.get(4)?,
                    template_kind: row.get(5)?,
                    ownership_scope: row.get(6)?,
                    folder_id: row.get(7)?,
                    folder_name: row.get(8)?,
                    note_count: row.get(9)?,
                    has_structured_notes: row.get::<_, bool>(10)?,
                })
            },
        )
        .map_err(|e| ParaError::Db(e.to_string()))
    }

    pub fn set_meeting_template(&self, meeting_id: &str, template_kind: &str) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET template_kind=?2 WHERE id=?1",
            params![meeting_id, template_kind],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn update_structured_note_summary(&self, meeting_id: &str, summary: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE meeting_structured_notes
             SET summary=?2, updated_at=?3
             WHERE meeting_id=?1",
            params![meeting_id, summary, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn update_structured_note_item(&self, item_id: i64, text: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE meeting_note_items
             SET text=?2, updated_at=?3
             WHERE id=?1",
            params![item_id, text, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn replace_structured_meeting_note(
        &self,
        meeting_id: &str,
        note: &StructuredMeetingNoteDraft,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        let mut conn = self.inner.lock().unwrap();
        let tx = conn
            .transaction()
            .map_err(|e| ParaError::Db(e.to_string()))?;

        tx.execute(
            "INSERT INTO meeting_structured_notes(
                meeting_id, template_kind, ownership_scope, summary, generated_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(meeting_id) DO UPDATE SET
                template_kind=excluded.template_kind,
                ownership_scope=excluded.ownership_scope,
                summary=excluded.summary,
                generated_at=excluded.generated_at,
                updated_at=excluded.updated_at",
            params![
                meeting_id,
                note.template_kind,
                note.ownership_scope,
                note.summary,
                now
            ],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;

        tx.execute(
            "DELETE FROM meeting_note_items WHERE meeting_id=?1",
            params![meeting_id],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;

        for section in &note.sections {
            for item in &section.items {
                tx.execute(
                    "INSERT INTO meeting_note_items(
                        meeting_id, section_kind, position, author_kind, text,
                        retrieval_confidence, evidence_count, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    params![
                        meeting_id,
                        section.kind,
                        item.position,
                        item.author_kind,
                        item.text,
                        item.retrieval_confidence,
                        item.evidence_count,
                        now
                    ],
                )
                .map_err(|e| ParaError::Db(e.to_string()))?;
                let item_id = tx.last_insert_rowid();
                for citation in &item.citations {
                    tx.execute(
                        "INSERT INTO meeting_note_item_citations(
                            note_item_id, segment_id, ts_ms, source, excerpt
                         ) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            item_id,
                            citation.segment_id,
                            citation.ts_ms,
                            citation.source,
                            citation.excerpt
                        ],
                    )
                    .map_err(|e| ParaError::Db(e.to_string()))?;
                }
            }
        }

        tx.commit().map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn get_structured_meeting_note(
        &self,
        meeting_id: &str,
    ) -> Result<Option<StructuredMeetingNote>> {
        let conn = self.inner.lock().unwrap();
        let root = conn
            .query_row(
                "SELECT meeting_id, template_kind, ownership_scope, summary, generated_at, updated_at
                 FROM meeting_structured_notes
                 WHERE meeting_id=?1",
                params![meeting_id],
                |row| {
                    Ok(StructuredMeetingNote {
                        meeting_id: row.get(0)?,
                        template_kind: row.get(1)?,
                        ownership_scope: row.get(2)?,
                        summary: row.get(3)?,
                        generated_at: row.get(4)?,
                        updated_at: row.get(5)?,
                        sections: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let Some(mut note) = root else {
            return Ok(None);
        };

        let mut item_stmt = conn
            .prepare(
                "SELECT id, section_kind, position, author_kind, text, retrieval_confidence, evidence_count
                 FROM meeting_note_items
                 WHERE meeting_id=?1
                 ORDER BY position ASC, id ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let item_rows = item_stmt
            .query_map(params![meeting_id], |row| {
                Ok(StructuredNoteItem {
                    id: row.get(0)?,
                    section_kind: row.get(1)?,
                    position: row.get(2)?,
                    author_kind: row.get(3)?,
                    text: row.get(4)?,
                    retrieval_confidence: row.get(5)?,
                    evidence_count: row.get(6)?,
                    citations: Vec::new(),
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut sections: Vec<StructuredNoteSection> = Vec::new();

        for row in item_rows {
            let mut item = row.map_err(|e| ParaError::Db(e.to_string()))?;
            let mut cite_stmt = conn
                .prepare(
                    "SELECT segment_id, ts_ms, source, excerpt
                     FROM meeting_note_item_citations
                     WHERE note_item_id=?1
                     ORDER BY id ASC",
                )
                .map_err(|e| ParaError::Db(e.to_string()))?;
            let cite_rows = cite_stmt
                .query_map(params![item.id], |cite_row| {
                    Ok(StructuredNoteCitation {
                        segment_id: cite_row.get(0)?,
                        ts_ms: cite_row.get(1)?,
                        source: cite_row.get(2)?,
                        excerpt: cite_row.get(3)?,
                    })
                })
                .map_err(|e| ParaError::Db(e.to_string()))?;

            for cite in cite_rows {
                item.citations
                    .push(cite.map_err(|e| ParaError::Db(e.to_string()))?);
            }

            if let Some(section) = sections.iter_mut().find(|s| s.kind == item.section_kind) {
                section.items.push(item);
            } else {
                sections.push(StructuredNoteSection {
                    kind: item.section_kind.clone(),
                    label: section_label(&item.section_kind).to_string(),
                    items: vec![item],
                });
            }
        }

        note.sections = sections;
        Ok(Some(note))
    }

    // ---- Meetings ----

    pub fn list_meetings(&self) -> Result<Vec<MeetingWithNotes>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT m.id,
                        m.provider,
                        m.started_at,
                        m.ended_at,
                        m.title,
                        m.template_kind,
                        m.folder_id,
                        f.name AS folder_name,
                        m.ownership_scope,
                        COUNT(n.id) AS note_count,
                        COALESCE(
                            (
                                SELECT summary
                                FROM meeting_structured_notes
                                WHERE meeting_id = m.id
                                LIMIT 1
                            ),
                            (
                            SELECT text
                            FROM notes
                            WHERE meeting_id = m.id
                            ORDER BY id DESC
                            LIMIT 1
                            )
                        ) AS note_preview,
                        EXISTS(
                            SELECT 1
                            FROM meeting_structured_notes msn
                            WHERE msn.meeting_id = m.id
                        ) AS has_structured_notes
                 FROM meetings m
                 LEFT JOIN folders f ON f.id = m.folder_id
                 LEFT JOIN notes n ON n.meeting_id = m.id
                 GROUP BY m.id, m.provider, m.started_at, m.ended_at, m.title, m.template_kind, m.folder_id, f.name, m.ownership_scope
                 ORDER BY m.started_at DESC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(MeetingWithNotes {
                    id: row.get(0)?,
                    provider: row.get(1)?,
                    started_at: row.get(2)?,
                    ended_at: row.get(3)?,
                    title: row.get(4)?,
                    template_kind: row.get(5)?,
                    folder_id: row.get(6)?,
                    folder_name: row.get(7)?,
                    ownership_scope: row.get(8)?,
                    note_count: row.get(9)?,
                    note_preview: row.get(10)?,
                    has_structured_notes: row.get::<_, bool>(11)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn get_notes_for_meeting(&self, meeting_id: &str) -> Result<Vec<NoteRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, meeting_id, ts_ms, text
                 FROM notes
                 WHERE meeting_id=?1
                 ORDER BY id ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map(params![meeting_id], |row| {
                Ok(NoteRow {
                    id: row.get(0)?,
                    meeting_id: row.get(1)?,
                    ts_ms: row.get(2)?,
                    text: row.get(3)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn list_all_notes(&self) -> Result<Vec<NoteRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, meeting_id, ts_ms, text
                 FROM notes
                 ORDER BY ts_ms DESC, id DESC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(NoteRow {
                    id: row.get(0)?,
                    meeting_id: row.get(1)?,
                    ts_ms: row.get(2)?,
                    text: row.get(3)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    // ---- Standalone notes ----

    pub fn create_standalone_note(&self, title: &str) -> Result<StandaloneNoteRow> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO standalone_notes(title, text, created_at, updated_at)
             VALUES (?1, '', ?2, ?2)",
            params![title, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;

        let id = conn.last_insert_rowid();
        Ok(StandaloneNoteRow {
            id,
            title: title.to_string(),
            text: String::new(),
            ownership_scope: "local_only".to_string(),
            folder_id: None,
            folder_name: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_standalone_note(&self, note_id: i64) -> Result<StandaloneNoteRow> {
        let conn = self.inner.lock().unwrap();
        conn.query_row(
            "SELECT sn.id, sn.title, sn.text, sn.ownership_scope, sn.folder_id, f.name, sn.created_at, sn.updated_at
             FROM standalone_notes sn
             LEFT JOIN folders f ON f.id = sn.folder_id
             WHERE sn.id=?1",
            params![note_id],
            |row| {
                Ok(StandaloneNoteRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    text: row.get(2)?,
                    ownership_scope: row.get(3)?,
                    folder_id: row.get(4)?,
                    folder_name: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .map_err(|e| ParaError::Db(e.to_string()))
    }

    pub fn update_standalone_note(&self, note_id: i64, title: &str, text: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE standalone_notes
             SET title=?2, text=?3, updated_at=?4
             WHERE id=?1",
            params![note_id, title, text, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn list_standalone_notes(&self) -> Result<Vec<StandaloneNoteRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT sn.id, sn.title, sn.text, sn.ownership_scope, sn.folder_id, f.name, sn.created_at, sn.updated_at
                 FROM standalone_notes sn
                 LEFT JOIN folders f ON f.id = sn.folder_id
                 ORDER BY sn.updated_at DESC, sn.id DESC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(StandaloneNoteRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    text: row.get(2)?,
                    ownership_scope: row.get(3)?,
                    folder_id: row.get(4)?,
                    folder_name: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn delete_standalone_note(&self, note_id: i64) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute("DELETE FROM standalone_notes WHERE id=?1", params![note_id])
            .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    // ---- Folders ----

    pub fn create_folder(
        &self,
        name: &str,
        kind: &str,
        color: Option<&str>,
        ownership_scope: &str,
        owner_user_id: Option<&str>,
        owner_org_id: Option<&str>,
    ) -> Result<FolderRow> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO folders(name, kind, color, ownership_scope, owner_user_id, owner_org_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                name,
                kind,
                color,
                ownership_scope,
                owner_user_id,
                owner_org_id,
                now
            ],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;

        let id = conn.last_insert_rowid();
        Ok(FolderRow {
            id,
            name: name.to_string(),
            kind: kind.to_string(),
            color: color.map(|s| s.to_string()),
            ownership_scope: ownership_scope.to_string(),
            owner_user_id: owner_user_id.map(|s| s.to_string()),
            owner_org_id: owner_org_id.map(|s| s.to_string()),
            meeting_count: 0,
            note_count: 0,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_folder(
        &self,
        folder_id: i64,
        name: &str,
        kind: &str,
        color: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE folders
             SET name=?2, kind=?3, color=?4, updated_at=?5
             WHERE id=?1",
            params![folder_id, name, kind, color, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn delete_folder(&self, folder_id: i64) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET folder_id=NULL WHERE folder_id=?1",
            params![folder_id],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        conn.execute(
            "UPDATE standalone_notes SET folder_id=NULL WHERE folder_id=?1",
            params![folder_id],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        conn.execute("DELETE FROM folders WHERE id=?1", params![folder_id])
            .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn list_folders(&self) -> Result<Vec<FolderRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT f.id,
                        f.name,
                        f.kind,
                        f.color,
                        f.ownership_scope,
                        f.owner_user_id,
                        f.owner_org_id,
                        (
                            SELECT COUNT(*) FROM meetings m WHERE m.folder_id = f.id
                        ) AS meeting_count,
                        (
                            SELECT COUNT(*) FROM standalone_notes sn WHERE sn.folder_id = f.id
                        ) AS note_count,
                        f.created_at,
                        f.updated_at
                 FROM folders f
                 ORDER BY f.updated_at DESC, f.id DESC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FolderRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    kind: row.get(2)?,
                    color: row.get(3)?,
                    ownership_scope: row.get(4)?,
                    owner_user_id: row.get(5)?,
                    owner_org_id: row.get(6)?,
                    meeting_count: row.get(7)?,
                    note_count: row.get(8)?,
                    created_at: row.get(9)?,
                    updated_at: row.get(10)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn assign_meeting_folder(&self, meeting_id: &str, folder_id: Option<i64>) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET folder_id=?2 WHERE id=?1",
            params![meeting_id, folder_id],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn assign_standalone_note_folder(
        &self,
        note_id: i64,
        folder_id: Option<i64>,
    ) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "UPDATE standalone_notes SET folder_id=?2 WHERE id=?1",
            params![note_id, folder_id],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn list_meetings_for_folder(&self, folder_id: i64) -> Result<Vec<MeetingWithNotes>> {
        Ok(self
            .list_meetings()?
            .into_iter()
            .filter(|meeting| meeting.folder_id == Some(folder_id))
            .collect())
    }

    pub fn list_standalone_notes_for_folder(
        &self,
        folder_id: i64,
    ) -> Result<Vec<StandaloneNoteRow>> {
        Ok(self
            .list_standalone_notes()?
            .into_iter()
            .filter(|note| note.folder_id == Some(folder_id))
            .collect())
    }

    pub fn list_org_shared_key_statuses(&self, org_id: i64) -> Result<Vec<OrgSharedKeyStatusRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT org_id, provider, updated_at
                 FROM org_shared_keys
                 WHERE org_id=?1
                 ORDER BY provider ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![org_id], |row| {
                Ok(OrgSharedKeyStatusRow {
                    org_id: row.get(0)?,
                    provider: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn upsert_org_shared_key_status(&self, org_id: i64, provider: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO org_shared_keys(org_id, provider, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(org_id, provider) DO UPDATE SET updated_at=excluded.updated_at",
            params![org_id, provider, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn delete_org_shared_key_status(&self, org_id: i64, provider: &str) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "DELETE FROM org_shared_keys WHERE org_id=?1 AND provider=?2",
            params![org_id, provider],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn get_session_context(&self) -> Result<SessionContextRow> {
        let conn = self.inner.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT mode, user_id, email, active_org_id, updated_at
                 FROM session_cache
                 WHERE id = 1",
                [],
                |row| {
                    Ok(SessionContextRow {
                        mode: row.get(0)?,
                        user_id: row.get(1)?,
                        email: row.get(2)?,
                        active_org_id: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| ParaError::Db(e.to_string()))?;

        Ok(row.unwrap_or(SessionContextRow {
            mode: "local_only".to_string(),
            user_id: None,
            email: None,
            active_org_id: None,
            updated_at: 0,
        }))
    }

    pub fn set_session_context(
        &self,
        mode: &str,
        user_id: Option<&str>,
        email: Option<&str>,
        active_org_id: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO session_cache(id, mode, user_id, email, active_org_id, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                mode=excluded.mode,
                user_id=excluded.user_id,
                email=excluded.email,
                active_org_id=excluded.active_org_id,
                updated_at=excluded.updated_at",
            params![mode, user_id, email, active_org_id, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    // ---- Retrieval ----

    pub fn search_segments_global(
        &self,
        query: &str,
        limit: i64,
        folder_id: Option<i64>,
    ) -> Result<Vec<SegmentSearchHit>> {
        let conn = self.inner.lock().unwrap();
        let sql = if folder_id.is_some() {
            r#"
            SELECT m.id, m.title, m.folder_id, f.name, s.id, s.source, s.ts_ms, s.text, s.confidence
            FROM segments_fts
            JOIN segments s ON s.id = segments_fts.rowid
            JOIN meetings m ON m.id = s.meeting_id
            LEFT JOIN folders f ON f.id = m.folder_id
            WHERE segments_fts MATCH ?1 AND m.folder_id = ?2
            ORDER BY bm25(segments_fts)
            LIMIT ?3
            "#
        } else {
            r#"
            SELECT m.id, m.title, m.folder_id, f.name, s.id, s.source, s.ts_ms, s.text, s.confidence
            FROM segments_fts
            JOIN segments s ON s.id = segments_fts.rowid
            JOIN meetings m ON m.id = s.meeting_id
            LEFT JOIN folders f ON f.id = m.folder_id
            WHERE segments_fts MATCH ?1
            ORDER BY bm25(segments_fts)
            LIMIT ?2
            "#
        };

        let mut out = Vec::new();
        if let Ok(mut stmt) = conn.prepare(sql) {
            let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<SegmentSearchHit> {
                Ok(SegmentSearchHit {
                    meeting_id: row.get(0)?,
                    meeting_title: row.get(1)?,
                    folder_id: row.get(2)?,
                    folder_name: row.get(3)?,
                    segment: SegmentRow {
                        id: row.get(4)?,
                        source: row.get(5)?,
                        ts_ms: row.get(6)?,
                        text: row.get(7)?,
                        confidence: row.get(8)?,
                    },
                })
            };

            let rows = if let Some(folder_id) = folder_id {
                stmt.query_map(params![query, folder_id, limit], mapper)
            } else {
                stmt.query_map(params![query, limit], mapper)
            }
            .map_err(|e| ParaError::Db(e.to_string()))?;

            for row in rows {
                out.push(row.map_err(|e| ParaError::Db(e.to_string()))?);
            }
            return Ok(out);
        }

        let like = query
            .replace(" OR ", " ")
            .replace(" AND ", " ")
            .replace('\"', "");
        let sql = if folder_id.is_some() {
            "SELECT m.id, m.title, m.folder_id, f.name, s.id, s.source, s.ts_ms, s.text, s.confidence
             FROM segments s
             JOIN meetings m ON m.id = s.meeting_id
             LEFT JOIN folders f ON f.id = m.folder_id
             WHERE s.text LIKE '%' || ?1 || '%' AND m.folder_id = ?2
             ORDER BY s.id DESC
             LIMIT ?3"
        } else {
            "SELECT m.id, m.title, m.folder_id, f.name, s.id, s.source, s.ts_ms, s.text, s.confidence
             FROM segments s
             JOIN meetings m ON m.id = s.meeting_id
             LEFT JOIN folders f ON f.id = m.folder_id
             WHERE s.text LIKE '%' || ?1 || '%'
             ORDER BY s.id DESC
             LIMIT ?2"
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<SegmentSearchHit> {
            Ok(SegmentSearchHit {
                meeting_id: row.get(0)?,
                meeting_title: row.get(1)?,
                folder_id: row.get(2)?,
                folder_name: row.get(3)?,
                segment: SegmentRow {
                    id: row.get(4)?,
                    source: row.get(5)?,
                    ts_ms: row.get(6)?,
                    text: row.get(7)?,
                    confidence: row.get(8)?,
                },
            })
        };
        let rows = if let Some(folder_id) = folder_id {
            stmt.query_map(params![like.trim(), folder_id, limit], mapper)
        } else {
            stmt.query_map(params![like.trim(), limit], mapper)
        }
        .map_err(|e| ParaError::Db(e.to_string()))?;
        for row in rows {
            out.push(row.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn search_structured_items_global(
        &self,
        query: &str,
        limit: i64,
        folder_id: Option<i64>,
    ) -> Result<Vec<StructuredItemSearchHit>> {
        let conn = self.inner.lock().unwrap();
        let sql = if folder_id.is_some() {
            "SELECT m.id, m.title, m.folder_id, f.name, i.id, i.section_kind, i.text, i.evidence_count
             FROM meeting_note_items i
             JOIN meetings m ON m.id = i.meeting_id
             LEFT JOIN folders f ON f.id = m.folder_id
             WHERE i.text LIKE '%' || ?1 || '%' AND m.folder_id = ?2
             ORDER BY i.updated_at DESC, i.id DESC
             LIMIT ?3"
        } else {
            "SELECT m.id, m.title, m.folder_id, f.name, i.id, i.section_kind, i.text, i.evidence_count
             FROM meeting_note_items i
             JOIN meetings m ON m.id = i.meeting_id
             LEFT JOIN folders f ON f.id = m.folder_id
             WHERE i.text LIKE '%' || ?1 || '%'
             ORDER BY i.updated_at DESC, i.id DESC
             LIMIT ?2"
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<StructuredItemSearchHit> {
            Ok(StructuredItemSearchHit {
                meeting_id: row.get(0)?,
                meeting_title: row.get(1)?,
                folder_id: row.get(2)?,
                folder_name: row.get(3)?,
                item_id: row.get(4)?,
                section_kind: row.get(5)?,
                text: row.get(6)?,
                evidence_count: row.get(7)?,
            })
        };
        let rows = if let Some(folder_id) = folder_id {
            stmt.query_map(params![query, folder_id, limit], mapper)
        } else {
            stmt.query_map(params![query, limit], mapper)
        }
        .map_err(|e| ParaError::Db(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn search_standalone_notes_global(
        &self,
        query: &str,
        limit: i64,
        folder_id: Option<i64>,
    ) -> Result<Vec<StandaloneNoteSearchHit>> {
        let conn = self.inner.lock().unwrap();
        let sql = if folder_id.is_some() {
            "SELECT sn.id, sn.title, sn.text, sn.folder_id, f.name
             FROM standalone_notes sn
             LEFT JOIN folders f ON f.id = sn.folder_id
             WHERE (sn.title LIKE '%' || ?1 || '%' OR sn.text LIKE '%' || ?1 || '%')
               AND sn.folder_id = ?2
             ORDER BY sn.updated_at DESC, sn.id DESC
             LIMIT ?3"
        } else {
            "SELECT sn.id, sn.title, sn.text, sn.folder_id, f.name
             FROM standalone_notes sn
             LEFT JOIN folders f ON f.id = sn.folder_id
             WHERE sn.title LIKE '%' || ?1 || '%' OR sn.text LIKE '%' || ?1 || '%'
             ORDER BY sn.updated_at DESC, sn.id DESC
             LIMIT ?2"
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<StandaloneNoteSearchHit> {
            Ok(StandaloneNoteSearchHit {
                note_id: row.get(0)?,
                title: row.get(1)?,
                text: row.get(2)?,
                folder_id: row.get(3)?,
                folder_name: row.get(4)?,
            })
        };
        let rows = if let Some(folder_id) = folder_id {
            stmt.query_map(params![query, folder_id, limit], mapper)
        } else {
            stmt.query_map(params![query, limit], mapper)
        }
        .map_err(|e| ParaError::Db(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    // ---- Participants ----

    pub fn add_participant(
        &self,
        name: &str,
        email: Option<&str>,
        source: &str,
    ) -> Result<ParticipantRow> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();

        if let Some(em) = email {
            if let Some(row) = conn
                .query_row(
                    "SELECT id, name, email, source, created_at
                     FROM participants
                     WHERE email=?1",
                    params![em],
                    |row| {
                        Ok(ParticipantRow {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            email: row.get(2)?,
                            source: row.get(3)?,
                            created_at: row.get(4)?,
                            org_name: None,
                            last_meeting_at: None,
                            meeting_count: 0,
                        })
                    },
                )
                .optional()
                .map_err(|e| ParaError::Db(e.to_string()))?
            {
                return Ok(row);
            }
        }

        conn.execute(
            "INSERT INTO participants(name, email, source, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![name, email, source, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;

        let id = conn.last_insert_rowid();
        Ok(ParticipantRow {
            id,
            name: name.to_string(),
            email: email.map(|s| s.to_string()),
            source: source.to_string(),
            created_at: now,
            org_name: None,
            last_meeting_at: None,
            meeting_count: 0,
        })
    }

    pub fn link_participant_meeting(&self, meeting_id: &str, participant_id: i64) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO meeting_participants(meeting_id, participant_id)
             VALUES (?1, ?2)",
            params![meeting_id, participant_id],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn list_participants_for_meeting(&self, meeting_id: &str) -> Result<Vec<ParticipantRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT p.id,
                        p.name,
                        p.email,
                        p.source,
                        p.created_at,
                        o.name AS org_name,
                        (
                            SELECT MAX(m2.started_at)
                            FROM meetings m2
                            JOIN meeting_participants mp2 ON mp2.meeting_id = m2.id
                            WHERE mp2.participant_id = p.id
                        ) AS last_meeting_at,
                        (
                            SELECT COUNT(*)
                            FROM meeting_participants mp2
                            WHERE mp2.participant_id = p.id
                        ) AS meeting_count
                 FROM participants p
                 JOIN meeting_participants mp ON mp.participant_id = p.id
                 LEFT JOIN participant_org po ON po.participant_id = p.id
                 LEFT JOIN organizations o ON o.id = po.org_id
                 WHERE mp.meeting_id = ?1
                 ORDER BY p.name ASC, p.id ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map(params![meeting_id], |row| {
                Ok(ParticipantRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    email: row.get(2)?,
                    source: row.get(3)?,
                    created_at: row.get(4)?,
                    org_name: row.get(5)?,
                    last_meeting_at: row.get(6)?,
                    meeting_count: row.get(7)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn list_all_participants(&self) -> Result<Vec<ParticipantRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT p.id,
                        p.name,
                        p.email,
                        p.source,
                        p.created_at,
                        o.name AS org_name,
                        (
                            SELECT MAX(m.started_at)
                            FROM meetings m
                            JOIN meeting_participants mp ON mp.meeting_id = m.id
                            WHERE mp.participant_id = p.id
                        ) AS last_meeting_at,
                        (
                            SELECT COUNT(*)
                            FROM meeting_participants mp
                            WHERE mp.participant_id = p.id
                        ) AS meeting_count
                 FROM participants p
                 LEFT JOIN participant_org po ON po.participant_id = p.id
                 LEFT JOIN organizations o ON o.id = po.org_id
                 ORDER BY p.name ASC, p.id ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ParticipantRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    email: row.get(2)?,
                    source: row.get(3)?,
                    created_at: row.get(4)?,
                    org_name: row.get(5)?,
                    last_meeting_at: row.get(6)?,
                    meeting_count: row.get(7)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    // ---- Organizations ----

    pub fn add_organization(&self, name: &str, domain: Option<&str>) -> Result<OrganizationRow> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();

        if let Some(d) = domain {
            if let Some(row) = conn
                .query_row(
                    "SELECT id, name, domain, created_at
                     FROM organizations
                     WHERE domain=?1",
                    params![d],
                    |row| {
                        Ok(OrganizationRow {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            domain: row.get(2)?,
                            created_at: row.get(3)?,
                            people_count: 0,
                            last_meeting_at: None,
                        })
                    },
                )
                .optional()
                .map_err(|e| ParaError::Db(e.to_string()))?
            {
                return Ok(row);
            }
        }

        conn.execute(
            "INSERT INTO organizations(name, domain, created_at)
             VALUES (?1, ?2, ?3)",
            params![name, domain, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;

        let id = conn.last_insert_rowid();
        Ok(OrganizationRow {
            id,
            name: name.to_string(),
            domain: domain.map(|s| s.to_string()),
            created_at: now,
            people_count: 0,
            last_meeting_at: None,
        })
    }

    pub fn link_participant_org(&self, participant_id: i64, org_id: i64) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO participant_org(participant_id, org_id)
             VALUES (?1, ?2)",
            params![participant_id, org_id],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }

    pub fn list_organizations(&self) -> Result<Vec<OrganizationRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT o.id,
                        o.name,
                        o.domain,
                        o.created_at,
                        (
                            SELECT COUNT(*)
                            FROM participant_org po
                            WHERE po.org_id = o.id
                        ) AS people_count,
                        (
                            SELECT MAX(m.started_at)
                            FROM meetings m
                            JOIN meeting_participants mp ON mp.meeting_id = m.id
                            JOIN participant_org po ON po.participant_id = mp.participant_id
                            WHERE po.org_id = o.id
                        ) AS last_meeting_at
                 FROM organizations o
                 ORDER BY o.name ASC, o.id ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(OrganizationRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    domain: row.get(2)?,
                    created_at: row.get(3)?,
                    people_count: row.get(4)?,
                    last_meeting_at: row.get(5)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn upsert_calendar_account(
        &self,
        provider: &str,
        email: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<CalendarAccountRow> {
        let now = Utc::now().timestamp();
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "INSERT INTO calendar_accounts(provider, email, display_name, connected_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(provider) DO UPDATE SET
               email=excluded.email,
               display_name=excluded.display_name,
               updated_at=excluded.updated_at",
            params![provider, email, display_name, now],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;

        Ok(CalendarAccountRow {
            provider: provider.to_string(),
            email: email.map(|s| s.to_string()),
            display_name: display_name.map(|s| s.to_string()),
            connected_at: now,
            updated_at: now,
        })
    }

    pub fn get_calendar_account(&self, provider: &str) -> Result<Option<CalendarAccountRow>> {
        let conn = self.inner.lock().unwrap();
        conn.query_row(
            "SELECT provider, email, display_name, connected_at, updated_at
             FROM calendar_accounts
             WHERE provider=?1",
            params![provider],
            |row| {
                Ok(CalendarAccountRow {
                    provider: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    connected_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|e| ParaError::Db(e.to_string()))
    }

    pub fn list_calendar_accounts(&self) -> Result<Vec<CalendarAccountRow>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT provider, email, display_name, connected_at, updated_at
                 FROM calendar_accounts
                 ORDER BY provider ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(CalendarAccountRow {
                    provider: row.get(0)?,
                    email: row.get(1)?,
                    display_name: row.get(2)?,
                    connected_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
    }

    pub fn delete_calendar_account(&self, provider: &str) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute(
            "DELETE FROM calendar_accounts WHERE provider=?1",
            params![provider],
        )
        .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
    }
}

fn format_segment(seg: &SegmentRow) -> String {
    let ts_sec = seg.ts_ms / 1000;
    let min = ts_sec / 60;
    let sec = ts_sec % 60;
    format!("[{:02}:{:02}] [{}] {}", min % 60, sec, seg.source, seg.text)
}

fn section_label(kind: &str) -> &'static str {
    match kind {
        "summary" => "Summary",
        "key_decisions" => "Key decisions",
        "action_items" => "Action items",
        "open_questions" => "Open questions",
        "risks_blockers" => "Risks / blockers",
        "quotes_highlights" => "Quotes / highlights",
        _ => "Notes",
    }
}

/// A row from the segments table.
#[derive(Debug, Clone, Serialize)]
pub struct SegmentRow {
    pub id: i64,
    pub source: String,
    pub ts_ms: i64,
    pub text: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct MeetingWithNotes {
    pub id: String,
    pub provider: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub title: Option<String>,
    pub template_kind: String,
    pub note_count: i64,
    pub note_preview: Option<String>,
    pub has_structured_notes: bool,
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
    pub ownership_scope: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingDetailRow {
    pub id: String,
    pub provider: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub title: Option<String>,
    pub template_kind: String,
    pub ownership_scope: String,
    pub note_count: i64,
    pub has_structured_notes: bool,
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NoteRow {
    pub id: i64,
    pub meeting_id: String,
    pub ts_ms: i64,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct ParticipantRow {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub source: String,
    pub created_at: i64,
    pub org_name: Option<String>,
    pub last_meeting_at: Option<i64>,
    pub meeting_count: i64,
}

#[derive(Debug, Serialize)]
pub struct OrganizationRow {
    pub id: i64,
    pub name: String,
    pub domain: Option<String>,
    pub created_at: i64,
    pub people_count: i64,
    pub last_meeting_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarAccountRow {
    pub provider: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub connected_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarConfigRow {
    pub provider: String,
    pub configured: bool,
    pub connected: bool,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpcomingCalendarEventRow {
    pub provider: String,
    pub account_email: Option<String>,
    pub external_id: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub organizer: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StandaloneNoteRow {
    pub id: i64,
    pub title: String,
    pub text: String,
    pub ownership_scope: String,
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub color: Option<String>,
    pub ownership_scope: String,
    pub owner_user_id: Option<String>,
    pub owner_org_id: Option<String>,
    pub meeting_count: i64,
    pub note_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContextRow {
    pub mode: String,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub active_org_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentSearchHit {
    pub meeting_id: String,
    pub meeting_title: Option<String>,
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
    pub segment: SegmentRow,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuredItemSearchHit {
    pub meeting_id: String,
    pub meeting_title: Option<String>,
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
    pub item_id: i64,
    pub section_kind: String,
    pub text: String,
    pub evidence_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StandaloneNoteSearchHit {
    pub note_id: i64,
    pub title: String,
    pub text: String,
    pub folder_id: Option<i64>,
    pub folder_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgSharedKeyStatusRow {
    pub org_id: i64,
    pub provider: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredMeetingNote {
    pub meeting_id: String,
    pub template_kind: String,
    pub ownership_scope: String,
    pub summary: String,
    pub generated_at: i64,
    pub updated_at: i64,
    pub sections: Vec<StructuredNoteSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredMeetingNoteDraft {
    pub template_kind: String,
    pub ownership_scope: String,
    pub summary: String,
    pub sections: Vec<StructuredNoteSectionDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredNoteSection {
    pub kind: String,
    pub label: String,
    pub items: Vec<StructuredNoteItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredNoteSectionDraft {
    pub kind: String,
    pub items: Vec<StructuredNoteItemDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredNoteItem {
    pub id: i64,
    pub section_kind: String,
    pub position: i64,
    pub author_kind: String,
    pub text: String,
    pub retrieval_confidence: Option<f64>,
    pub evidence_count: i64,
    pub citations: Vec<StructuredNoteCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredNoteItemDraft {
    pub position: i64,
    pub author_kind: String,
    pub text: String,
    pub retrieval_confidence: Option<f64>,
    pub evidence_count: i64,
    pub citations: Vec<StructuredNoteCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredNoteCitation {
    pub segment_id: i64,
    pub ts_ms: i64,
    pub source: String,
    pub excerpt: String,
}

/// Run schema migrations.
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS meetings(
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            started_at INTEGER NOT NULL,
            ended_at INTEGER,
            title TEXT,
            source_hint TEXT,
            template_kind TEXT NOT NULL DEFAULT 'generic',
            ownership_scope TEXT NOT NULL DEFAULT 'local_only'
        );

        CREATE TABLE IF NOT EXISTS segments(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            meeting_id TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'SYS',
            ts_ms INTEGER NOT NULL,
            text TEXT NOT NULL,
            confidence REAL,
            FOREIGN KEY(meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS notes(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            meeting_id TEXT NOT NULL,
            ts_ms INTEGER NOT NULL,
            text TEXT NOT NULL,
            FOREIGN KEY(meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS export_cache(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            meeting_id TEXT NOT NULL,
            format TEXT NOT NULL,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS participants(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            email TEXT,
            source TEXT NOT NULL DEFAULT 'manual',
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS meeting_participants(
            meeting_id TEXT NOT NULL,
            participant_id INTEGER NOT NULL,
            PRIMARY KEY(meeting_id, participant_id),
            FOREIGN KEY(meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
            FOREIGN KEY(participant_id) REFERENCES participants(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS organizations(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            domain TEXT UNIQUE,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS folders(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'project',
            color TEXT,
            ownership_scope TEXT NOT NULL DEFAULT 'local_only',
            owner_user_id TEXT,
            owner_org_id TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS org_shared_keys(
            org_id INTEGER NOT NULL,
            provider TEXT NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY(org_id, provider),
            FOREIGN KEY(org_id) REFERENCES organizations(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS session_cache(
            id INTEGER PRIMARY KEY CHECK (id = 1),
            mode TEXT NOT NULL DEFAULT 'local_only',
            user_id TEXT,
            email TEXT,
            active_org_id TEXT,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS calendar_accounts(
            provider TEXT PRIMARY KEY,
            email TEXT,
            display_name TEXT,
            connected_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS participant_org(
            participant_id INTEGER NOT NULL,
            org_id INTEGER NOT NULL,
            PRIMARY KEY(participant_id, org_id),
            FOREIGN KEY(participant_id) REFERENCES participants(id) ON DELETE CASCADE,
            FOREIGN KEY(org_id) REFERENCES organizations(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS standalone_notes(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            text TEXT NOT NULL DEFAULT '',
            ownership_scope TEXT NOT NULL DEFAULT 'local_only',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS meeting_structured_notes(
            meeting_id TEXT PRIMARY KEY,
            template_kind TEXT NOT NULL DEFAULT 'generic',
            ownership_scope TEXT NOT NULL DEFAULT 'local_only',
            summary TEXT NOT NULL DEFAULT '',
            generated_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY(meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS meeting_note_items(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            meeting_id TEXT NOT NULL,
            section_kind TEXT NOT NULL,
            position INTEGER NOT NULL,
            author_kind TEXT NOT NULL,
            text TEXT NOT NULL,
            retrieval_confidence REAL,
            evidence_count INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY(meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS meeting_note_item_citations(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            note_item_id INTEGER NOT NULL,
            segment_id INTEGER NOT NULL,
            ts_ms INTEGER NOT NULL,
            source TEXT NOT NULL,
            excerpt TEXT NOT NULL,
            FOREIGN KEY(note_item_id) REFERENCES meeting_note_items(id) ON DELETE CASCADE,
            FOREIGN KEY(segment_id) REFERENCES segments(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_segments_meeting_id ON segments(meeting_id);
        CREATE INDEX IF NOT EXISTS idx_notes_meeting_id ON notes(meeting_id);
        CREATE INDEX IF NOT EXISTS idx_meeting_participants_meeting_id ON meeting_participants(meeting_id);
        CREATE INDEX IF NOT EXISTS idx_meeting_participants_participant_id ON meeting_participants(participant_id);
        CREATE INDEX IF NOT EXISTS idx_participant_org_participant_id ON participant_org(participant_id);
        CREATE INDEX IF NOT EXISTS idx_participant_org_org_id ON participant_org(org_id);
        CREATE INDEX IF NOT EXISTS idx_participants_email ON participants(email);
        CREATE INDEX IF NOT EXISTS idx_organizations_domain ON organizations(domain);
        CREATE INDEX IF NOT EXISTS idx_folders_kind ON folders(kind);
        CREATE INDEX IF NOT EXISTS idx_meeting_note_items_meeting_id ON meeting_note_items(meeting_id);
        CREATE INDEX IF NOT EXISTS idx_meeting_note_items_section_kind ON meeting_note_items(section_kind);
        CREATE INDEX IF NOT EXISTS idx_meeting_note_item_citations_item_id ON meeting_note_item_citations(note_item_id);
    "#,
    )
    .map_err(|e| ParaError::Db(e.to_string()))?;

    ensure_column(
        conn,
        "meetings",
        "template_kind",
        "ALTER TABLE meetings ADD COLUMN template_kind TEXT NOT NULL DEFAULT 'generic'",
    )?;
    ensure_column(
        conn,
        "meetings",
        "ownership_scope",
        "ALTER TABLE meetings ADD COLUMN ownership_scope TEXT NOT NULL DEFAULT 'local_only'",
    )?;
    ensure_column(
        conn,
        "standalone_notes",
        "ownership_scope",
        "ALTER TABLE standalone_notes ADD COLUMN ownership_scope TEXT NOT NULL DEFAULT 'local_only'",
    )?;
    ensure_column(
        conn,
        "meetings",
        "folder_id",
        "ALTER TABLE meetings ADD COLUMN folder_id INTEGER",
    )?;
    ensure_column(
        conn,
        "meetings",
        "owner_user_id",
        "ALTER TABLE meetings ADD COLUMN owner_user_id TEXT",
    )?;
    ensure_column(
        conn,
        "meetings",
        "owner_org_id",
        "ALTER TABLE meetings ADD COLUMN owner_org_id TEXT",
    )?;
    ensure_column(
        conn,
        "standalone_notes",
        "folder_id",
        "ALTER TABLE standalone_notes ADD COLUMN folder_id INTEGER",
    )?;
    ensure_column(
        conn,
        "standalone_notes",
        "owner_user_id",
        "ALTER TABLE standalone_notes ADD COLUMN owner_user_id TEXT",
    )?;
    ensure_column(
        conn,
        "standalone_notes",
        "owner_org_id",
        "ALTER TABLE standalone_notes ADD COLUMN owner_org_id TEXT",
    )?;
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_meetings_folder_id ON meetings(folder_id);
        CREATE INDEX IF NOT EXISTS idx_standalone_notes_folder_id ON standalone_notes(folder_id);
    "#,
    )
    .map_err(|e| ParaError::Db(e.to_string()))?;

    // FTS5 virtual table + triggers (best effort).
    // If FTS5 is not available in the bundled build, queries fall back to LIKE.
    let _ = conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS segments_fts USING fts5(
            text,
            content='segments',
            content_rowid='id'
        );

        CREATE TRIGGER IF NOT EXISTS segments_ai AFTER INSERT ON segments BEGIN
            INSERT INTO segments_fts(rowid, text) VALUES (new.id, new.text);
        END;

        CREATE TRIGGER IF NOT EXISTS segments_ad AFTER DELETE ON segments BEGIN
            INSERT INTO segments_fts(segments_fts, rowid, text)
            VALUES('delete', old.id, old.text);
        END;

        CREATE TRIGGER IF NOT EXISTS segments_au AFTER UPDATE ON segments BEGIN
            INSERT INTO segments_fts(segments_fts, rowid, text)
            VALUES('delete', old.id, old.text);
            INSERT INTO segments_fts(rowid, text) VALUES (new.id, new.text);
        END;
    "#,
    );

    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, sql: &str) -> Result<()> {
    let pragma = format!("PRAGMA table_info({})", table);
    let mut stmt = conn
        .prepare(&pragma)
        .map_err(|e| ParaError::Db(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| ParaError::Db(e.to_string()))?;

    for row in rows {
        if row.map_err(|e| ParaError::Db(e.to_string()))? == column {
            return Ok(());
        }
    }

    conn.execute(sql, [])
        .map_err(|e| ParaError::Db(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_query_meeting() {
        let store = LocalStore::open_memory().unwrap();
        let mid = store.create_meeting("deepgram").unwrap();
        assert!(!mid.is_empty());

        let ts = chrono::Utc::now().timestamp_millis();
        store
            .insert_segment(&mid, "SYS", ts, "hello world", Some(0.95))
            .unwrap();
        store
            .insert_segment(&mid, "MIC", ts + 1000, "how are you", None)
            .unwrap();

        let segs = store.segments_for_meeting(&mid).unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].text, "hello world");
        assert_eq!(segs[0].source, "SYS");
        assert_eq!(segs[1].text, "how are you");
        assert_eq!(segs[1].source, "MIC");
    }

    #[test]
    fn test_notes_and_generate() {
        let store = LocalStore::open_memory().unwrap();
        let mid = store.create_meeting("assemblyai").unwrap();

        store.insert_note(&mid, "discuss roadmap").unwrap();

        let ts = chrono::Utc::now().timestamp_millis();
        store
            .insert_segment(&mid, "SYS", ts, "let us discuss the roadmap", None)
            .unwrap();

        store.generate_meeting_notes(&mid).unwrap();

        let notes = store.notes_for_meeting(&mid).unwrap();
        assert!(notes.len() >= 2);
        assert!(notes.last().unwrap().contains("# Meeting Notes"));
    }

    #[test]
    fn test_fts_query() {
        let store = LocalStore::open_memory().unwrap();
        let mid = store.create_meeting("mock").unwrap();

        let ts = chrono::Utc::now().timestamp_millis();
        store
            .insert_segment(&mid, "SYS", ts, "important decision about the launch", None)
            .unwrap();
        store
            .insert_segment(
                &mid,
                "SYS",
                ts + 1000,
                "we need to finalize the budget",
                None,
            )
            .unwrap();
        store
            .insert_segment(&mid, "SYS", ts + 2000, "the weather is nice today", None)
            .unwrap();

        let results = store
            .top_segments_for_query(&mid, "decision OR budget", 10)
            .unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_end_meeting() {
        let store = LocalStore::open_memory().unwrap();
        let mid = store.create_meeting("mock").unwrap();
        store.end_meeting(&mid).unwrap();
        store.update_meeting_title(&mid, "Test Meeting").unwrap();
    }

    #[test]
    fn test_standalone_notes_crud() {
        let store = LocalStore::open_memory().unwrap();
        let note = store.create_standalone_note("Draft").unwrap();
        store
            .update_standalone_note(note.id, "Updated", "Body")
            .unwrap();
        let fetched = store.get_standalone_note(note.id).unwrap();
        assert_eq!(fetched.title, "Updated");
        assert_eq!(fetched.text, "Body");
        assert_eq!(store.list_standalone_notes().unwrap().len(), 1);
        store.delete_standalone_note(note.id).unwrap();
        assert!(store.list_standalone_notes().unwrap().is_empty());
    }

    #[test]
    fn test_structured_note_roundtrip() {
        let store = LocalStore::open_memory().unwrap();
        let meeting_id = store.create_meeting("mock").unwrap();
        store
            .insert_segment(&meeting_id, "SYS", 1000, "pilot next Friday", Some(0.9))
            .unwrap();

        let draft = StructuredMeetingNoteDraft {
            template_kind: "generic".into(),
            ownership_scope: "local_only".into(),
            summary: "A concise summary".into(),
            sections: vec![StructuredNoteSectionDraft {
                kind: "key_decisions".into(),
                items: vec![StructuredNoteItemDraft {
                    position: 0,
                    author_kind: "ai".into(),
                    text: "Ship the pilot next Friday".into(),
                    retrieval_confidence: Some(0.8),
                    evidence_count: 1,
                    citations: vec![StructuredNoteCitation {
                        segment_id: 1,
                        ts_ms: 1000,
                        source: "SYS".into(),
                        excerpt: "pilot next Friday".into(),
                    }],
                }],
            }],
        };

        store
            .replace_structured_meeting_note(&meeting_id, &draft)
            .unwrap();
        let fetched = store
            .get_structured_meeting_note(&meeting_id)
            .unwrap()
            .unwrap();
        assert_eq!(fetched.summary, "A concise summary");
        assert_eq!(fetched.sections.len(), 1);
        assert_eq!(fetched.sections[0].items[0].citations.len(), 1);
    }

    #[test]
    fn test_folder_assignment_and_listing() {
        let store = LocalStore::open_memory().unwrap();
        let folder = store
            .create_folder("Acme", "client", None, "local_only", None, None)
            .unwrap();
        let meeting_id = store.create_meeting("mock").unwrap();
        let note = store.create_standalone_note("Draft").unwrap();

        store
            .assign_meeting_folder(&meeting_id, Some(folder.id))
            .unwrap();
        store
            .assign_standalone_note_folder(note.id, Some(folder.id))
            .unwrap();

        let meetings = store.list_meetings_for_folder(folder.id).unwrap();
        let notes = store.list_standalone_notes_for_folder(folder.id).unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].folder_id, Some(folder.id));
    }

    #[test]
    fn test_session_context_roundtrip() {
        let store = LocalStore::open_memory().unwrap();
        store
            .set_session_context(
                "signed_in_personal",
                Some("user-1"),
                Some("a@example.com"),
                Some("org-1"),
            )
            .unwrap();
        let session = store.get_session_context().unwrap();
        assert_eq!(session.mode, "signed_in_personal");
        assert_eq!(session.user_id.as_deref(), Some("user-1"));
        assert_eq!(session.active_org_id.as_deref(), Some("org-1"));
    }
}
