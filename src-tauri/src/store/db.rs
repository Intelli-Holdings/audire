use crate::error::{ParaError, Result};

use chrono::Utc;
use directories::ProjectDirs;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
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
        let proj = ProjectDirs::from("com", "paraaudio", "Para-audio")
            .ok_or_else(|| ParaError::Db("failed to resolve project dirs".into()))?;
        let dir = proj.data_dir();
        std::fs::create_dir_all(dir).map_err(|e| ParaError::Db(e.to_string()))?;
        let db_path = dir.join("para_audio.db");

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
                "SELECT source, ts_ms, text FROM segments \
                 WHERE meeting_id=?1 ORDER BY id ASC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![meeting_id], |row| {
                Ok(SegmentRow {
                    source: row.get(0)?,
                    ts_ms: row.get(1)?,
                    text: row.get(2)?,
                })
            })
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| ParaError::Db(e.to_string()))?);
        }
        Ok(out)
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
                    .query_map(params![meeting_id, query, limit], |row| row.get::<_, String>(0))
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
                    .query_map(params![meeting_id, query, limit], |row| row.get::<_, String>(0))
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

        let export_dir = app_data_dir.join("exports");
        std::fs::create_dir_all(&export_dir).map_err(|e| ParaError::Db(e.to_string()))?;
        let out_path = export_dir.join(format!("meeting-{}.md", meeting_id));

        let mut md = String::new();
        md.push_str("# Para-audio Export\n\n");
        md.push_str(&format!("Meeting: `{}`\n\n", meeting_id));

        md.push_str("## Notes\n\n");
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
                        COUNT(n.id) AS note_count,
                        (
                            SELECT text
                            FROM notes
                            WHERE meeting_id = m.id
                            ORDER BY id DESC
                            LIMIT 1
                        ) AS note_preview
                 FROM meetings m
                 LEFT JOIN notes n ON n.meeting_id = m.id
                 GROUP BY m.id, m.provider, m.started_at, m.ended_at, m.title
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
                    note_count: row.get(5)?,
                    note_preview: row.get(6)?,
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
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get_standalone_note(&self, note_id: i64) -> Result<StandaloneNoteRow> {
        let conn = self.inner.lock().unwrap();
        conn.query_row(
            "SELECT id, title, text, created_at, updated_at
             FROM standalone_notes
             WHERE id=?1",
            params![note_id],
            |row| {
                Ok(StandaloneNoteRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    text: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
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
                "SELECT id, title, text, created_at, updated_at
                 FROM standalone_notes
                 ORDER BY updated_at DESC, id DESC",
            )
            .map_err(|e| ParaError::Db(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(StandaloneNoteRow {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    text: row.get(2)?,
                    created_at: row.get(3)?,
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

    pub fn delete_standalone_note(&self, note_id: i64) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute("DELETE FROM standalone_notes WHERE id=?1", params![note_id])
            .map_err(|e| ParaError::Db(e.to_string()))?;
        Ok(())
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
}

fn format_segment(seg: &SegmentRow) -> String {
    let ts_sec = seg.ts_ms / 1000;
    let min = ts_sec / 60;
    let sec = ts_sec % 60;
    format!("[{:02}:{:02}] [{}] {}", min % 60, sec, seg.source, seg.text)
}

/// A row from the segments table.
#[derive(Debug, Serialize)]
pub struct SegmentRow {
    pub source: String,
    pub ts_ms: i64,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct MeetingWithNotes {
    pub id: String,
    pub provider: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub title: Option<String>,
    pub note_count: i64,
    pub note_preview: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct StandaloneNoteRow {
    pub id: i64,
    pub title: String,
    pub text: String,
    pub created_at: i64,
    pub updated_at: i64,
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
            source_hint TEXT
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
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_segments_meeting_id ON segments(meeting_id);
        CREATE INDEX IF NOT EXISTS idx_notes_meeting_id ON notes(meeting_id);
        CREATE INDEX IF NOT EXISTS idx_meeting_participants_meeting_id ON meeting_participants(meeting_id);
        CREATE INDEX IF NOT EXISTS idx_meeting_participants_participant_id ON meeting_participants(participant_id);
        CREATE INDEX IF NOT EXISTS idx_participant_org_participant_id ON participant_org(participant_id);
        CREATE INDEX IF NOT EXISTS idx_participant_org_org_id ON participant_org(org_id);
        CREATE INDEX IF NOT EXISTS idx_participants_email ON participants(email);
        CREATE INDEX IF NOT EXISTS idx_organizations_domain ON organizations(domain);
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
            .insert_segment(&mid, "SYS", ts + 1000, "we need to finalize the budget", None)
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
}
