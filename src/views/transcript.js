// Transcript view — Granola-style note view with floating recording card
// Recording: title + meta chips at top, note area, floating transcript card, bottom bar

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { showToast } from '../toast.js';

let appState = null;
let onCaptureStop = null;
let onNavigateHome = null;
let meetingSegments = [];
let currentStructuredNote = null;
let currentMeeting = null;
let meetingUserNotes = [];
let showingMeetingDetail = false;

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

function formatDateTime(tsMs) {
  return new Date(tsMs).toLocaleTimeString('en-US', {
    hour: '2-digit', minute: '2-digit', second: '2-digit',
  });
}

function formatShortTime(tsMs) {
  const totalSeconds = Math.floor(tsMs / 1000);
  const minutes = Math.floor(totalSeconds / 60) % 60;
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
}

function formatClockTime(value) {
  return value.toLocaleTimeString('en-US', {
    hour: '2-digit', minute: '2-digit', hour12: false,
  });
}

function formatRelativeDate(tsSeconds) {
  if (!tsSeconds) return '';
  const date = new Date(tsSeconds * 1000);
  const now = new Date();
  const diffDays = Math.floor(
    (new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() -
     new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()) / 86400000
  );
  if (diffDays === 0) return 'Today';
  if (diffDays === 1) return 'Yesterday';
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

function getElapsedStr() {
  if (!appState.captureStartedAt) return '00:00';
  const elapsed = Math.floor((Date.now() - appState.captureStartedAt) / 1000);
  const m = Math.floor(elapsed / 60);
  const s = elapsed % 60;
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

export function initTranscriptView(state, callbacks = {}) {
  appState = state;
  onCaptureStop = callbacks.onCaptureStop || null;
  onNavigateHome = callbacks.onNavigateHome || null;
  registerASRListeners();
}

async function registerASRListeners() {
  await listen('asr:audio_level', (event) => {
    appState.captureAudioLevel = Number(event.payload?.level || 0);
    const fill = document.getElementById('audio-level-fill');
    if (fill) fill.style.width = `${Math.round(appState.captureAudioLevel * 100)}%`;
  });

  await listen('asr:partial', (event) => {
    console.log('[audire] partial:', event.payload);
    appState.partialText = event.payload?.text || '';
    updateLiveStrip();
  });

  await listen('asr:final', (event) => {
    console.log('[audire] final:', event.payload);
    const t = event.payload?.text || '';
    const ts = event.payload?.ts_ms || Date.now();
    const prov = event.payload?.provider || '';
    const formatted = Boolean(event.payload?.formatted);
    if (t) {
      appState.finals.push({ text: t, ts_ms: ts, provider: prov, formatted });
      appState.partialText = '';
      // Force full rebuild so the committed line appears and partial is cleared
      const stripText = document.getElementById('live-transcript-text');
      if (stripText) {
        stripText.innerHTML = buildTranscriptHtml();
        stripText.scrollTop = stripText.scrollHeight;
      }
    }
  });

  await listen('asr:status', (event) => {
    console.log('[audire] status:', event.payload);
    appState.captureStatus = event.payload?.status || '';
  });

  await listen('asr:lifecycle', (event) => {
    console.log('[audire] lifecycle:', event.payload);
    const nextState = event.payload?.state || '';
    const eventMeetingId = event.payload?.meeting_id || '';
    const message = event.payload?.message || '';

    if (eventMeetingId && appState.meetingId && eventMeetingId !== appState.meetingId) return;

    if (nextState === 'running') {
      appState.captureValidated = true;
      return;
    }

    if (nextState === 'stopped') {
      appState.isCapturing = false;
      appState.captureStartedAt = null;
      if (onCaptureStop) onCaptureStop();
      if (appState.currentView === 'transcript' && showingMeetingDetail) {
        renderMeetingDetail();
      }
      return;
    }

    if (nextState === 'error') {
      appState.isCapturing = false;
      appState.captureStartedAt = null;
      if (onCaptureStop) onCaptureStop();
      if (message) showToast(message, 'error');
    }
  });
}

export async function renderTranscriptView() {
  const container = document.getElementById('view-transcript');
  if (!container) return;

  if (appState.isCapturing || appState.meetingId) {
    showingMeetingDetail = true;
    await renderMeetingDetail();
  } else {
    // No meeting selected — go back to home
    showingMeetingDetail = false;
    if (onNavigateHome) onNavigateHome();
  }
}

async function renderMeetingDetail() {
  const container = document.getElementById('view-transcript');
  meetingSegments = [];
  meetingUserNotes = [];
  currentStructuredNote = null;
  currentMeeting = null;

  if (appState.meetingId && !appState.isCapturing) {
    try {
      const [detail, participants] = await Promise.all([
        invoke('get_meeting_detail', { meetingId: appState.meetingId }),
        invoke('list_participants', { meetingId: appState.meetingId }),
      ]);
      currentMeeting = detail.meeting;
      meetingSegments = detail.segments || [];
      meetingUserNotes = detail.user_notes || [];
      currentStructuredNote = detail.structured_note || null;
    } catch (e) {
      console.error('Meeting detail load error:', e);
    }
  }

  const title = currentMeeting?.title || (appState.meetingId ? 'Meeting Notes' : 'Live Session');
  const startedAt = currentMeeting?.started_at
    ? new Date(currentMeeting.started_at * 1000)
    : null;
  const dateChip = startedAt
    ? startedAt.toLocaleDateString('en-US', { day: 'numeric', month: 'short' })
    : 'Now';

  const userNotesText = meetingUserNotes.map(n => n.text).join('\n\n');

  // Build floating transcript card content
  let transcriptCardHtml = '';
  if (appState.isCapturing) {
    transcriptCardHtml = `
      <div class="floating-transcript-card" id="floating-transcript">
        <div class="floating-transcript-header">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="color:var(--color-text-muted);">
            <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/>
            <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
          </svg>
          <div class="floating-transcript-icons">
            <button class="sidebar-icon-btn" id="transcript-settings-btn" title="Settings">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/></svg>
            </button>
            <button class="sidebar-icon-btn" id="transcript-collapse-btn" title="Collapse">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="5" y1="12" x2="19" y2="12"/></svg>
            </button>
          </div>
        </div>
        <div class="floating-transcript-body" id="floating-transcript-body">
          <div class="floating-transcript-consent">
            Always get consent when transcribing others.
          </div>
          <div class="floating-transcript-elapsed" id="elapsed-timer">${getElapsedStr()}</div>
          <div class="audio-level-bar" id="audio-level-bar" style="height:3px;background:var(--color-surface-2);border-radius:2px;margin:var(--space-1) var(--space-4);overflow:hidden;">
            <div id="audio-level-fill" style="height:100%;width:0%;background:var(--color-accent);border-radius:2px;transition:width 100ms ease;"></div>
          </div>
          <div class="floating-transcript-text" id="live-transcript-text">
            ${buildTranscriptHtml()}
          </div>
        </div>
      </div>
    `;
  }

  // Generate notes button (only show when recording is done and no structured note yet)
  let generateBtnHtml = '';
  if (!appState.isCapturing && appState.meetingId && !currentStructuredNote && meetingSegments.length > 0) {
    generateBtnHtml = `
      <div style="display:flex;justify-content:center;padding:var(--space-3) 0;">
        <button class="btn-primary" id="generate-notes-btn" style="display:flex;align-items:center;gap:var(--space-2);">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m12 3-1.912 5.813a2 2 0 0 1-1.275 1.275L3 12l5.813 1.912a2 2 0 0 1 1.275 1.275L12 21l1.912-5.813a2 2 0 0 1 1.275-1.275L21 12l-5.813-1.912a2 2 0 0 1-1.275-1.275L12 3Z"/></svg>
          Generate notes
        </button>
      </div>
    `;
  }

  // Enhanced notes below the user notes area
  let enhancedHtml = '';
  if (currentStructuredNote) {
    enhancedHtml = `
      <div style="border-top:1px solid var(--color-surface-2);margin-top:var(--space-6);padding-top:var(--space-4);">
        <div class="pane-label">AI enhanced</div>
        <div class="enhanced-content text-ai" id="enhanced-notes-content">
          ${renderStructuredNotes(currentStructuredNote)}
        </div>
      </div>
    `;
  }

  container.innerHTML = `
    <div style="flex:1;overflow-y:auto;display:flex;flex-direction:column;align-items:center;position:relative;">
      <div style="width:100%;max-width:680px;padding:var(--space-8) var(--space-6);flex:1;">
        <!-- Title -->
        <input class="meeting-title-input" id="session-title" value="${escapeHtml(title)}" placeholder="Untitled session"
          style="font-family:var(--font-display);font-size:var(--text-h1);font-weight:var(--weight-normal);margin-bottom:var(--space-3);" />

        <!-- Meta chips -->
        <div style="display:flex;gap:var(--space-2);margin-bottom:var(--space-6);flex-wrap:wrap;">
          <span class="badge-subtle" style="display:flex;align-items:center;gap:4px;">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="18" rx="2" ry="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/></svg>
            ${escapeHtml(dateChip)}
          </span>
          <span class="badge-subtle" style="display:flex;align-items:center;gap:4px;">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
            Me
          </span>
          <button class="badge-subtle" id="add-to-folder-chip" style="cursor:pointer;display:flex;align-items:center;gap:4px;">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
            Add to folder
          </button>
        </div>

        <!-- Notes textarea -->
        <textarea
          id="user-notes-input"
          class="notes-textarea text-user"
          placeholder="Write your notes here..."
          spellcheck="true"
          style="min-height:300px;"
        >${escapeHtml(userNotesText)}</textarea>

        ${enhancedHtml}
      </div>

      <!-- Floating transcript card (when capturing) -->
      ${transcriptCardHtml}
    </div>

    ${generateBtnHtml}

    <!-- Bottom bar -->
    <div class="view-bottom-bar" style="justify-content:flex-start;">
      <button class="capture-bottom-btn" id="capture-toggle-btn" aria-label="Audio source">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="2" y="6" width="4" height="12" rx="1"/>
          <rect x="10" y="4" width="4" height="16" rx="1"/>
          <rect x="18" y="8" width="4" height="8" rx="1"/>
        </svg>
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="margin-left:-4px;"><path d="m6 9 6 6 6-6"/></svg>
      </button>
      <button class="capture-resume-btn ${appState.isCapturing ? 'is-recording' : (meetingSegments.length || appState.finals.length ? 'is-paused' : 'is-idle')}"
              id="capture-resume-btn"
              data-capture-state="${appState.isCapturing ? 'recording' : (meetingSegments.length || appState.finals.length ? 'paused' : 'idle')}">
        ${appState.isCapturing
          ? '<span class="capture-dot" aria-hidden="true"></span> Stop'
          : ((meetingSegments.length || appState.finals.length) ? 'Resume' : 'Start recording')}
      </button>
      <input type="text" class="ask-input" id="transcript-ask-input" placeholder="Ask anything" style="flex:1;" />
      <button class="recipe-shortcut-btn" id="transcript-recipe-btn">/ List recent todos</button>
    </div>
  `;

  bindMeetingDetailEvents();

  // Start elapsed timer update
  if (appState.isCapturing) {
    const timerEl = document.getElementById('elapsed-timer');
    if (timerEl) {
      const interval = setInterval(() => {
        if (!appState.isCapturing) {
          clearInterval(interval);
          return;
        }
        timerEl.textContent = getElapsedStr();
      }, 1000);
    }
  }
}

function renderStructuredNotes(note) {
  let html = '';

  if (note.summary) {
    html += `
      <label class="enhanced-summary-label">Summary</label>
      <textarea id="structured-summary-input" class="enhanced-summary-input" rows="4">${escapeHtml(note.summary)}</textarea>
    `;
  }

  if (note.sections?.length) {
    for (const section of note.sections) {
      html += `<div class="enhanced-section">`;
      html += `<div class="enhanced-section-title">${escapeHtml(section.label)}</div>`;
      if (section.items?.length) {
        for (const item of section.items) {
          html += `
            <div class="enhanced-item">
              <div class="enhanced-item-text" contenteditable="true" data-item-id="${item.id}">${escapeHtml(item.text)}</div>
              <div class="enhanced-item-meta">
                <span class="badge-subtle">${escapeHtml(item.author_kind)}</span>
                <span>${item.evidence_count} source${item.evidence_count === 1 ? '' : 's'}</span>
                ${item.retrieval_confidence ? `<span>confidence ${(item.retrieval_confidence * 100).toFixed(0)}%</span>` : ''}
              </div>
              ${item.citations?.length ? `
                <div class="citation-row">
                  ${item.citations.map(c => `
                    <button class="citation-chip" data-segment-jump="${c.segment_id}">
                      ${escapeHtml(formatShortTime(c.ts_ms))} \u00B7 ${escapeHtml(c.excerpt.slice(0, 60))}
                    </button>
                  `).join('')}
                </div>
              ` : ''}
            </div>
          `;
        }
      } else {
        html += `<p class="text-muted text-sm">No items yet.</p>`;
      }
      html += `</div>`;
    }
  }

  return html || '<p class="text-muted text-sm">Structured notes are empty.</p>';
}

function buildTranscriptHtml() {
  const persistedRows = meetingSegments.map(seg => ({
    id: seg.id, ts_ms: seg.ts_ms, text: seg.text, kind: 'formatted-final',
  }));
  const persistedKeys = new Set(persistedRows.map(r => `${r.ts_ms}:${r.text}`));
  const liveRows = appState.finals
    .map(f => ({
      id: null, ts_ms: f.ts_ms, text: f.text,
      kind: f.formatted ? 'formatted-final' : 'plain-final',
    }))
    .filter(r => !persistedKeys.has(`${r.ts_ms}:${r.text}`));

  const all = [...persistedRows, ...liveRows].sort((a, b) => a.ts_ms - b.ts_ms);

  let html = '';
  for (const row of all) {
    html += `<div class="transcript-line ${row.kind}" ${row.id ? `data-transcript-segment="${row.id}"` : ''}>`;
    html += `<span class="transcript-line-time">${escapeHtml(formatDateTime(row.ts_ms))}</span>`;
    html += escapeHtml(row.text);
    html += `</div>`;
  }
  if (appState.partialText) {
    html += `<div class="transcript-line partial" id="live-partial-line">${escapeHtml(appState.partialText)}</div>`;
  }
  if (!all.length && !appState.partialText) {
    html = appState.isCapturing
      ? `Listening\u2026${appState.captureStatus ? '<div style="font-size:var(--text-xs);color:var(--color-text-muted);margin-top:var(--space-1);">' + escapeHtml(appState.captureStatus) + '</div>' : ''}`
      : 'No transcript yet.';
  }
  return html;
}

function updateLiveStrip() {
  const stripText = document.getElementById('live-transcript-text');
  if (!stripText) return;

  // Fast-path: if only the partial changed, update just the partial element
  // instead of rebuilding the entire transcript (avoids flicker/layout thrash).
  const partialEl = document.getElementById('live-partial-line');
  if (partialEl && appState.partialText) {
    partialEl.textContent = appState.partialText;
    stripText.scrollTop = stripText.scrollHeight;
    return;
  }

  // Full rebuild needed (new final arrived or partial appeared/disappeared)
  stripText.innerHTML = buildTranscriptHtml();
  stripText.scrollTop = stripText.scrollHeight;
}

function bindMeetingDetailEvents() {
  // Title editing
  document.getElementById('session-title')?.addEventListener('blur', async (e) => {
    const nextTitle = e.target.value.trim();
    if (!appState.meetingId || !nextTitle) return;
    try {
      await invoke('update_meeting_title', { meetingId: appState.meetingId, title: nextTitle });
    } catch (err) {
      showToast('Failed to update title', 'error');
    }
  });

  // Capture toggle — three distinct states driven by button data-capture-state
  //   "idle"      → Start recording
  //   "recording" → Stop
  //   "paused"    → Resume (start a new capture run on the same meeting context)
  document.getElementById('capture-resume-btn')?.addEventListener('click', async () => {
    const btn = document.getElementById('capture-resume-btn');
    const state = btn?.dataset.captureState || (appState.isCapturing ? 'recording' : 'idle');

    if (state === 'recording') {
      appState.isCapturing = false;
      try {
        await invoke('stop_capture', { meetingId: appState.meetingId });
        showToast('Recording stopped', 'success');
        renderMeetingDetail();
      } catch (err) {
        showToast('Error stopping capture: ' + err, 'error');
      }
      return;
    }

    // "idle" or "paused" — both start a new capture. We distinguish the toast
    // so users know whether they're beginning fresh or resuming.
    const provider = await autoSelectProvider();
    appState.isCapturing = true;
    try {
      const resp = await invoke('start_capture', {
        provider,
        mode: 'system',
        includeMic: true,
        targetProcess: null,
      });
      appState.meetingId = resp.meeting_id;
      appState.captureStartedAt = Date.now();
      appState.finals.length = 0;
      appState.partialText = '';
      showToast(state === 'paused' ? 'Recording resumed' : 'Recording started', 'success');
      renderMeetingDetail();
    } catch (err) {
      appState.isCapturing = false;
      showToast('Could not start capture: ' + err, 'error');
    }
  });

  // Generate notes
  document.getElementById('generate-notes-btn')?.addEventListener('click', async () => {
    if (!appState.meetingId) return;
    try {
      currentStructuredNote = await invoke('generate_structured_meeting_notes', {
        meetingId: appState.meetingId, templateKind: null,
      });
      renderMeetingDetail();
    } catch (err) {
      showToast('Failed to generate notes: ' + err, 'error');
    }
  });

  // Save user notes on blur
  document.getElementById('user-notes-input')?.addEventListener('blur', async () => {
    const text = document.getElementById('user-notes-input')?.value.trim();
    if (!appState.meetingId || !text) return;
    try {
      await invoke('append_note', { meetingId: appState.meetingId, text });
    } catch (e) {
      console.error('Append note error:', e);
    }
  });

  // Structured note summary editing
  document.getElementById('structured-summary-input')?.addEventListener('blur', async (e) => {
    if (!appState.meetingId || !currentStructuredNote) return;
    const summary = e.target.value.trim();
    try {
      await invoke('update_structured_note_summary', { meetingId: appState.meetingId, summary });
      currentStructuredNote.summary = summary;
    } catch (err) {
      console.error('Summary update error:', err);
    }
  });

  // Structured note item editing
  document.querySelectorAll('[data-item-id]').forEach(el => {
    el.addEventListener('blur', async () => {
      const itemId = parseInt(el.dataset.itemId);
      const text = el.textContent.trim();
      if (!text) return;
      try {
        await invoke('update_structured_note_item', { itemId, text });
      } catch (err) {
        console.error('Item update error:', err);
      }
    });
  });

  // Citation segment jumps
  document.querySelectorAll('[data-segment-jump]').forEach(el => {
    el.addEventListener('click', () => {
      const segmentId = parseInt(el.dataset.segmentJump);
      const line = document.querySelector(`[data-transcript-segment="${segmentId}"]`);
      if (line) {
        line.scrollIntoView({ behavior: 'smooth', block: 'center' });
        line.classList.add('flash-segment');
        setTimeout(() => line.classList.remove('flash-segment'), 1400);
      }
    });
  });

  // Ask input in bottom bar
  const askInput = document.getElementById('transcript-ask-input');
  askInput?.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      const msg = askInput.value.trim();
      if (!msg) return;
      askInput.value = '';

      // Check if it's a recipe command
      const recipeMatch = msg.match(/^\/(\w+)/);
      if (recipeMatch && appState.meetingId) {
        try {
          const resp = await invoke('run_recipe', {
            meetingId: appState.meetingId,
            recipeId: recipeMatch[1].toLowerCase(),
          });
          showToast(resp.text?.slice(0, 100) || 'Done', 'success');
        } catch (err) {
          showToast('Error: ' + err, 'error');
        }
        return;
      }

      // Otherwise use ask_audire
      try {
        let hasLlm = false;
        try {
          const hasAnthropic = await invoke('has_api_key', { provider: 'anthropic' });
          const hasOpenai = await invoke('has_api_key', { provider: 'openai' });
          hasLlm = hasAnthropic || hasOpenai;
        } catch { /* ignore */ }

        const command = hasLlm ? 'ask_audire_llm' : 'ask_audire';
        const resp = await invoke(command, {
          query: msg,
          scope: 'all',
          meetingId: appState.meetingId || null,
          folderId: null,
        });
        showToast(resp.answer?.slice(0, 100) || 'Done', 'success');
      } catch (err) {
        showToast('Error: ' + err, 'error');
      }
    }
  });

  // Recipe shortcut
  document.getElementById('transcript-recipe-btn')?.addEventListener('click', () => {
    if (askInput) {
      askInput.value = '/todos';
      askInput.focus();
    }
  });

  // Collapse transcript card
  document.getElementById('transcript-collapse-btn')?.addEventListener('click', () => {
    const card = document.getElementById('floating-transcript');
    if (card) card.style.display = card.style.display === 'none' ? '' : 'none';
  });
}

// Auto-select best available ASR provider (no UI selector)
async function autoSelectProvider() {
  const providers = ['assemblyai', 'deepgram'];
  for (const provider of providers) {
    try {
      const hasKey = await invoke('has_api_key', { provider });
      if (hasKey) return provider;
    } catch { /* continue */ }
  }
  return 'assemblyai'; // fallback
}
