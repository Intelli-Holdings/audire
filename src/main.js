import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './style.css';

// ---- State ----
let currentView = 'home';
let meetingId = null;
let isCapturing = false;
const finals = [];
let partialText = '';
let currentStandaloneNoteId = null;
const keyStatusCache = {};

// ---- Elements ----
const content = document.getElementById('content');
const searchModal = document.getElementById('searchModal');
const searchInput = document.getElementById('searchInput');
const captureBar = document.getElementById('captureBar');
const captureStatus = document.getElementById('captureStatus');
const captureStopBtn = document.getElementById('captureStopBtn');

// ---- Event listeners from Tauri ----
await listen('asr:partial', (event) => {
  partialText = event.payload?.text || '';
  updateTranscript();
});

await listen('asr:final', (event) => {
  const t = event.payload?.text || '';
  const ts = event.payload?.ts_ms || Date.now();
  const prov = event.payload?.provider || '';
  if (t) {
    finals.push({ text: t, ts_ms: ts, provider: prov });
    partialText = '';
    updateTranscript();
  }
});

await listen('asr:status', (event) => {
  const s = event.payload?.status || '';
  if (s && captureStatus) {
    captureStatus.textContent = s;
  }
});

// ---- Search modal ----
document.getElementById('searchTrigger').addEventListener('click', openSearch);
document.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
    e.preventDefault();
    openSearch();
  }
  if (e.key === 'Escape') closeSearch();
});
searchModal.addEventListener('click', (e) => {
  if (e.target === searchModal) closeSearch();
});

function openSearch() {
  searchModal.classList.remove('hidden');
  searchInput.focus();
  searchInput.value = '';
}
function closeSearch() {
  searchModal.classList.add('hidden');
}

// ---- Navigation ----
document.querySelectorAll('[data-view]').forEach(el => {
  el.addEventListener('click', (e) => {
    e.preventDefault();
    const view = el.dataset.view;
    navigate(view);
  });
});

window.addEventListener('hashchange', () => {
  const hash = location.hash.slice(1) || 'home';
  navigate(hash, false);
});

function navigate(view, pushHash = true) {
  currentView = view;
  if (pushHash) location.hash = view;

  // Update sidebar active state
  document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
  const active = document.querySelector(`[data-view="${view}"]`);
  if (active) active.classList.add('active');

  renderView(view);
}

// ---- New click handlers ----
document.getElementById('userProfile')?.addEventListener('click', () => navigate('settings'));
document.getElementById('newNoteBtn')?.addEventListener('click', async () => {
  try {
    const note = await invoke('create_standalone_note', { title: 'Untitled' });
    currentStandaloneNoteId = note.id;
    navigate('note-editor');
  } catch (e) {
    console.error('Create note error:', e);
  }
});

// ---- Capture bar ----
captureStopBtn.addEventListener('click', stopCapture);

async function startCapture(provider, opts = {}) {
  if (isCapturing) return;
  finals.length = 0;
  partialText = '';

  try {
    const resp = await invoke('start_capture', {
      provider,
      mode: opts.mode || 'system',
      includeMic: opts.includeMic !== false,
      targetProcess: opts.targetProcess || null,
    });
    meetingId = resp.meeting_id;
    isCapturing = true;
    captureBar.classList.remove('hidden');
    captureStatus.textContent = `Capturing (${provider})`;
    navigate('meeting');
  } catch (e) {
    console.error(e);
    alert(`Failed to start: ${e}`);
  }
}

async function stopCapture() {
  if (!isCapturing || !meetingId) return;
  try {
    await invoke('stop_capture', { meetingId });
  } catch (e) {
    console.error(e);
  } finally {
    isCapturing = false;
    captureBar.classList.add('hidden');
    captureStatus.textContent = 'Idle';
  }
}

async function exportMeeting() {
  if (!meetingId) return;
  try {
    const resp = await invoke('export', { meetingId, format: 'md' });
    alert(`Exported to: ${resp.path}`);
  } catch (e) {
    console.error('Export error:', e);
    alert(`Export failed: ${e}`);
  }
}

// ---- Views ----
function renderView(view) {
  switch (view) {
    case 'home': renderHome(); break;
    case 'meeting': renderMeeting(); break;
    case 'chat': renderChat(); break;
    case 'shared': renderShared(); break;
    case 'notes': renderNotes(); break;
    case 'people': renderPeople(); break;
    case 'companies': renderCompanies(); break;
    case 'recipes': renderRecipes(); break;
    case 'settings': renderSettings(); break;
    case 'note-editor': renderNoteEditor(); break;
    default: renderHome();
  }
}

async function renderHome() {
  const today = new Date();
  const dateStr = today.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  const dayName = today.toLocaleDateString('en-US', { weekday: 'short' });

  // Fetch recent meetings
  let meetings = [];
  try {
    meetings = await invoke('list_meetings');
  } catch (e) {
    console.error('list_meetings error:', e);
  }

  const recentHtml = meetings.length > 0
    ? meetings.slice(0, 5).map(m => {
        const d = new Date(m.started_at * 1000);
        const timeStr = d.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' });
        const title = m.title || `${m.provider} session`;
        return `
          <div class="meeting-item" data-mid="${escapeHtml(m.id)}">
            <div>
              <div class="meeting-title">${escapeHtml(title)}</div>
              <div class="meeting-time">${timeStr} &middot; ${m.note_count} note${m.note_count !== 1 ? 's' : ''}</div>
            </div>
          </div>`;
      }).join('')
    : `<div class="empty-state">
        <div class="empty-state-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
        </div>
        <h3>No meetings yet</h3>
        <p>Start a capture session to transcribe your first meeting.</p>
      </div>`;

  content.innerHTML = `
    <div class="view">
      <div class="capture-card">
        <h3>Start a capture session</h3>
        <div class="capture-row">
          <div class="select-wrap">
            <select id="providerSelect">
              <option value="deepgram">Deepgram (Flux v2)</option>
              <option value="assemblyai">AssemblyAI (U3 Pro)</option>
              <option value="mock">Mock (offline test)</option>
            </select>
          </div>
          <button class="btn btn-primary" id="startBtn" ${isCapturing ? 'disabled' : ''}>
            ${isCapturing ? 'Capturing...' : 'Start capture'}
          </button>
          ${isCapturing ? `<button class="btn btn-danger" id="viewMeetingBtn">View live session</button>` : ''}
        </div>
        <div class="capture-options">
          <label class="toggle-label">
            <input type="checkbox" id="includeMicToggle" checked />
            <span>Include microphone</span>
          </label>
          <label class="toggle-label">
            <input type="checkbox" id="systemModeToggle" checked disabled />
            <span>System audio (loopback)</span>
          </label>
        </div>
        <p class="status-line mt-16">Keys: set via <a href="#settings" style="color:var(--accent)">Settings</a>, environment variables, or OS keyring</p>
      </div>

      <div class="view-header">
        <h1>Coming up</h1>
      </div>

      <div class="coming-up">
        <div class="date-group">
          <div class="date-label">
            <div class="day">${today.getDate()}</div>
            <div class="month-day">${dateStr}<br/>${dayName}</div>
          </div>
          <div class="meetings-col">
            <div class="meeting-item" onclick="window.location.hash='meeting'">
              <div>
                <div class="meeting-title">Quick capture session</div>
                <div class="meeting-time">Start anytime -- captures mic + system audio</div>
              </div>
            </div>
            <p class="text-muted" style="padding: 8px 14px; font-size: 13px;">
              Audire captures audio in the background during your Google Meet, Teams, or Zoom calls.
              No bots join your meeting -- just local audio capture.
            </p>
          </div>
        </div>
      </div>

      <div style="margin-top: 32px;">
        <h2 style="font-size: 16px; font-weight: 600; margin-bottom: 12px; color: var(--text-secondary);">Recent</h2>
        ${recentHtml}
      </div>
    </div>
  `;

  document.getElementById('startBtn')?.addEventListener('click', () => {
    const provider = document.getElementById('providerSelect').value;
    const includeMic = document.getElementById('includeMicToggle')?.checked ?? true;
    startCapture(provider, { includeMic });
  });
  document.getElementById('viewMeetingBtn')?.addEventListener('click', () => navigate('meeting'));

  // Click on recent meeting items
  document.querySelectorAll('[data-mid]').forEach(el => {
    el.addEventListener('click', () => {
      meetingId = el.dataset.mid;
      navigate('meeting');
    });
  });
}

function renderMeeting() {
  content.innerHTML = `
    <div class="meeting-detail" style="padding-bottom: 320px;">
      <div class="meeting-detail-header">
        <h1>${meetingId ? 'Live Session' : 'Meeting Notes'}</h1>
        <div class="meeting-badges">
          <span class="badge">Today</span>
          ${isCapturing ? '<span class="badge recording-badge">Recording</span>' : ''}
        </div>
        <div class="meeting-actions">
          <button class="btn btn-ghost" id="exportBtn">Export MD</button>
        </div>
      </div>

      <div class="notes-editor" contenteditable="true" id="notesEditor"></div>

      ${isCapturing ? `
      <div class="transcript-panel" id="transcriptPanel">
        <div class="transcript-header" id="transcriptToggle">
          <h3>Live Transcript</h3>
          <span style="font-size: 12px; color: var(--text-muted);">Click to toggle</span>
        </div>
        <div class="transcript-body" id="transcriptBody">
          <div id="transcriptContent"></div>
        </div>
      </div>
      ` : `
      <div class="transcript-panel" id="transcriptPanel">
        <div class="transcript-header">
          <h3>Transcript (${finals.length} segments)</h3>
        </div>
        <div class="transcript-body" id="transcriptBody">
          <div id="transcriptContent"></div>
        </div>
      </div>
      `}

      <div class="ask-bar" id="askBar">
        <input type="text" placeholder="Ask anything" id="askInput" />
        <button class="recipe-chip" id="runSummaryBtn">
          <span class="chip-icon">/</span>
          Summary
        </button>
        <button class="recipe-chip" onclick="window.location.hash='recipes'">
          <span class="chip-icon">/</span>
          See all
        </button>
      </div>
    </div>
  `;

  // Transcript toggle
  document.getElementById('transcriptToggle')?.addEventListener('click', () => {
    document.getElementById('transcriptPanel')?.classList.toggle('collapsed');
  });

  // Export button
  document.getElementById('exportBtn')?.addEventListener('click', exportMeeting);

  // Save notes on blur
  const editor = document.getElementById('notesEditor');
  if (editor) {
    editor.addEventListener('blur', async () => {
      const text = editor.innerText.trim();
      if (text && meetingId) {
        try {
          await invoke('append_note', { meetingId, text });
        } catch (e) { console.error('Note save error:', e); }
      }
    });
  }

  // Summary recipe
  document.getElementById('runSummaryBtn')?.addEventListener('click', async () => {
    if (!meetingId) return;
    try {
      const out = await invoke('run_recipe', { meetingId, recipeId: 'summary' });
      const editor = document.getElementById('notesEditor');
      if (editor) editor.innerHTML = `<pre style="white-space: pre-wrap;">${escapeHtml(out.text)}</pre>`;
    } catch (e) {
      console.error('Recipe error:', e);
    }
  });

  updateTranscript();
}

function renderChat() {
  content.innerHTML = `
    <div class="view">
      <div class="view-header">
        <h1>Ask anything</h1>
      </div>

      <div class="chat-input-area">
        <div class="chat-input-header">
          <span class="scope-tag active" data-scope="notes">My notes</span>
          <span class="scope-tag" data-scope="meetings">All meetings</span>
        </div>
        <textarea class="chat-textarea" id="chatTextarea" placeholder="What decisions were made?" rows="2"></textarea>
        <div class="chat-input-footer">
          <div class="model-selector">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/></svg>
            LLM (requires feature flag)
          </div>
          <button class="btn-primary-sm" id="chatSendBtn">Send</button>
        </div>
      </div>

      <div id="chatResponse" style="margin-bottom: 20px;"></div>

      <h3 style="font-size: 13px; color: var(--text-secondary); margin-bottom: 12px;">Recipes</h3>
      <div style="display: flex; flex-wrap: wrap; gap: 8px;">
        <button class="recipe-chip"><span class="chip-icon">/</span> List recent todos</button>
        <button class="recipe-chip"><span class="chip-icon">/</span> Write weekly recap</button>
        <button class="recipe-chip"><span class="chip-icon">/</span> Summarize meeting</button>
        <button class="recipe-chip" onclick="window.location.hash='recipes'"><span class="chip-icon">#</span> See all</button>
      </div>
    </div>
  `;

  // Scope tag toggling
  document.querySelectorAll('.scope-tag[data-scope]').forEach(tag => {
    tag.addEventListener('click', () => {
      document.querySelectorAll('.scope-tag[data-scope]').forEach(t => t.classList.remove('active'));
      tag.classList.add('active');
    });
  });

  // Send button
  const sendBtn = document.getElementById('chatSendBtn');
  const textarea = document.getElementById('chatTextarea');

  async function sendChat() {
    if (!meetingId) {
      document.getElementById('chatResponse').innerHTML =
        '<p class="text-muted" style="padding:12px;">Start a capture session first to use chat.</p>';
      return;
    }
    const text = textarea.value.trim();
    if (!text) return;
    try {
      const out = await invoke('run_recipe', { meetingId, recipeId: 'summary' });
      document.getElementById('chatResponse').innerHTML =
        `<div class="settings-section"><pre style="white-space:pre-wrap;font-size:13px;color:var(--text-primary);">${escapeHtml(out.text)}</pre></div>`;
    } catch (e) {
      document.getElementById('chatResponse').innerHTML =
        `<p class="status-line error">Error: ${escapeHtml(String(e))}</p>`;
    }
  }

  sendBtn?.addEventListener('click', sendChat);
  textarea?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendChat();
    }
  });
}

function renderShared() {
  content.innerHTML = `
    <div class="view">
      <div class="view-header">
        <h1>Shared with me</h1>
        <p>Notes that others have shared with you will appear here.</p>
      </div>
      <div class="empty-state">
        <div class="empty-state-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>
        </div>
        <h3>No shared notes yet</h3>
        <p>Sharing is coming soon. In the meantime, check out <a href="#notes" style="color:var(--accent)">My Notes</a>.</p>
      </div>
    </div>
  `;
}

async function renderNotes() {
  content.innerHTML = `
    <div class="view">
      <div class="view-header">
        <h1>My notes</h1>
      </div>
      <p class="text-muted">Loading...</p>
    </div>
  `;

  let meetings = [];
  let standaloneNotes = [];
  try {
    [meetings, standaloneNotes] = await Promise.all([
      invoke('list_meetings'),
      invoke('list_standalone_notes'),
    ]);
  } catch (e) {
    console.error('Notes load error:', e);
  }

  let html = '<div class="view"><div class="view-header"><h1>My notes</h1></div>';

  // Standalone notes section
  if (standaloneNotes.length > 0) {
    html += '<div class="notes-sub-header"><h2>Standalone Notes</h2></div>';
    html += '<div class="notes-list">';
    for (const n of standaloneNotes) {
      const d = new Date(n.updated_at * 1000);
      const dateStr = d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
      html += `
        <div class="note-card" data-standalone-id="${n.id}">
          <div class="note-card-title">${escapeHtml(n.title)}</div>
          <div class="note-card-meta">${dateStr}</div>
          <div class="note-card-preview">${escapeHtml(n.text.slice(0, 120))}</div>
        </div>`;
    }
    html += '</div>';
  }

  // Meeting notes section
  const meetingsWithNotes = meetings.filter(m => m.note_count > 0);
  if (meetingsWithNotes.length > 0) {
    html += '<div class="notes-sub-header" style="margin-top:24px;"><h2>Meeting Notes</h2></div>';
    html += '<div class="notes-list">';
    for (const m of meetingsWithNotes) {
      const d = new Date(m.started_at * 1000);
      const dateStr = d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
      const title = m.title || `${m.provider} session`;
      html += `
        <div class="note-card" data-meeting-note-id="${escapeHtml(m.id)}">
          <div class="note-card-title">${escapeHtml(title)}</div>
          <div class="note-card-meta">${dateStr} &middot; ${m.note_count} note${m.note_count !== 1 ? 's' : ''}</div>
          <div class="note-card-preview">${escapeHtml((m.note_preview || '').slice(0, 120))}</div>
        </div>`;
    }
    html += '</div>';
  }

  if (standaloneNotes.length === 0 && meetingsWithNotes.length === 0) {
    html += `
      <div class="empty-state">
        <div class="empty-state-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
          </div>
        <h3>No notes yet</h3>
        <p>Click the pencil icon to create a note, or start a capture session.</p>
      </div>`;
  }

  html += '</div>';
  content.innerHTML = html;

  // Handlers for standalone note cards
  document.querySelectorAll('[data-standalone-id]').forEach(el => {
    el.addEventListener('click', () => {
      currentStandaloneNoteId = parseInt(el.dataset.standaloneId);
      navigate('note-editor');
    });
  });

  // Handlers for meeting note cards
  document.querySelectorAll('[data-meeting-note-id]').forEach(el => {
    el.addEventListener('click', () => {
      meetingId = el.dataset.meetingNoteId;
      navigate('meeting');
    });
  });
}

async function renderPeople() {
  content.innerHTML = `
    <div class="view">
      <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px;">
        <h1 style="font-size: 28px; font-weight: 700;">People</h1>
      </div>
      <p class="text-muted">Loading...</p>
    </div>
  `;

  let people = [];
  try {
    people = await invoke('list_all_participants');
  } catch (e) {
    console.error('list_all_participants error:', e);
  }

  let tbody = '';
  for (const p of people) {
    const initial = p.name.charAt(0).toUpperCase();
    const colors = ['green', 'blue', 'purple'];
    const color = colors[p.id % colors.length];
    const lastMeeting = p.last_meeting_at
      ? new Date(p.last_meeting_at * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
      : '-';
    tbody += `
      <tr>
        <td>
          <div class="person-cell">
            <div class="person-avatar ${color}">${escapeHtml(initial)}</div>
            <div>
              <div class="person-name">${escapeHtml(p.name)}</div>
              <div class="person-email">${escapeHtml(p.email || '')}</div>
            </div>
          </div>
        </td>
        <td class="text-muted">${escapeHtml(p.org_name || '-')}</td>
        <td class="text-muted">${lastMeeting}</td>
        <td>${p.meeting_count}</td>
      </tr>`;
  }

  if (people.length === 0) {
    tbody = `<tr><td colspan="4" class="text-muted" style="text-align:center;padding:24px;">No people added yet.</td></tr>`;
  }

  content.innerHTML = `
    <div class="view">
      <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px;">
        <h1 style="font-size: 28px; font-weight: 700;">People</h1>
      </div>
      <table class="data-table">
        <thead>
          <tr>
            <th>Person</th>
            <th>Company</th>
            <th>Last meeting</th>
            <th>Meetings</th>
          </tr>
        </thead>
        <tbody>${tbody}</tbody>
      </table>

      <div class="inline-form" id="addPersonForm">
        <input type="text" id="personName" placeholder="Name" style="flex:1;" />
        <input type="text" id="personEmail" placeholder="Email (optional)" style="flex:1;" />
        <button class="btn-primary-sm" id="addPersonBtn">Add person</button>
      </div>
      <p class="text-muted mt-16" style="font-size: 13px;">People from your meetings will appear here as you capture sessions.</p>
    </div>
  `;

  document.getElementById('addPersonBtn')?.addEventListener('click', async () => {
    const name = document.getElementById('personName')?.value.trim();
    if (!name) return;
    const email = document.getElementById('personEmail')?.value.trim() || null;
    try {
      await invoke('add_participant', { meetingId: null, name, email });
      renderPeople(); // re-render
    } catch (e) {
      console.error('add_participant error:', e);
    }
  });
}

async function renderCompanies() {
  content.innerHTML = `
    <div class="view">
      <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px;">
        <h1 style="font-size: 28px; font-weight: 700;">Companies</h1>
      </div>
      <p class="text-muted">Loading...</p>
    </div>
  `;

  let orgs = [];
  try {
    orgs = await invoke('list_organizations');
  } catch (e) {
    console.error('list_organizations error:', e);
  }

  let tbody = '';
  for (const o of orgs) {
    const lastMeeting = o.last_meeting_at
      ? new Date(o.last_meeting_at * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
      : '-';
    tbody += `
      <tr>
        <td>
          <div class="person-cell">
            <div class="person-avatar blue">${escapeHtml(o.name.charAt(0).toUpperCase())}</div>
            <div>
              <div class="person-name">${escapeHtml(o.name)}</div>
              <div class="person-email">${escapeHtml(o.domain || '')}</div>
            </div>
          </div>
        </td>
        <td>${o.people_count}</td>
        <td class="text-muted">${lastMeeting}</td>
      </tr>`;
  }

  if (orgs.length === 0) {
    tbody = `<tr><td colspan="3" class="text-muted" style="text-align:center;padding:24px;">No companies added yet.</td></tr>`;
  }

  content.innerHTML = `
    <div class="view">
      <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 20px;">
        <h1 style="font-size: 28px; font-weight: 700;">Companies</h1>
      </div>
      <table class="data-table">
        <thead>
          <tr>
            <th>Company</th>
            <th>People</th>
            <th>Last meeting</th>
          </tr>
        </thead>
        <tbody>${tbody}</tbody>
      </table>

      <div class="inline-form" id="addOrgForm">
        <input type="text" id="orgName" placeholder="Company name" style="flex:1;" />
        <input type="text" id="orgDomain" placeholder="Domain (optional)" style="flex:1;" />
        <button class="btn-primary-sm" id="addOrgBtn">Add company</button>
      </div>
      <p class="text-muted mt-16" style="font-size: 13px;">Companies of people you meet will appear here.</p>
    </div>
  `;

  document.getElementById('addOrgBtn')?.addEventListener('click', async () => {
    const name = document.getElementById('orgName')?.value.trim();
    if (!name) return;
    const domain = document.getElementById('orgDomain')?.value.trim() || null;
    try {
      await invoke('add_organization', { name, domain });
      renderCompanies(); // re-render
    } catch (e) {
      console.error('add_organization error:', e);
    }
  });
}

async function renderSettings() {
  const providers = [
    { id: 'deepgram', label: 'Deepgram', desc: 'Flux v2 streaming ASR' },
    { id: 'assemblyai', label: 'AssemblyAI', desc: 'U3 Pro streaming ASR' },
    { id: 'openai', label: 'OpenAI', desc: 'LLM recipes (feature-gated)' },
    { id: 'anthropic', label: 'Anthropic', desc: 'LLM recipes (feature-gated)' },
  ];

  // Check key status for all providers
  for (const p of providers) {
    try {
      keyStatusCache[p.id] = await invoke('has_api_key', { provider: p.id });
    } catch {
      keyStatusCache[p.id] = false;
    }
  }

  let keyRows = '';
  for (const p of providers) {
    const has = keyStatusCache[p.id];
    keyRows += `
      <div class="key-row">
        <span class="provider-label">${p.label}</span>
        <span class="key-status ${has ? 'configured' : 'missing'}">${has ? 'Configured' : 'Not set'}</span>
        <input type="password" class="key-input" id="key-${p.id}" placeholder="${p.desc}" />
        <button class="btn-primary-sm" data-save-key="${p.id}">Save</button>
        ${has ? `<button class="btn-danger-sm" data-delete-key="${p.id}">Delete</button>` : ''}
      </div>`;
  }

  content.innerHTML = `
    <div class="view">
      <div class="view-header">
        <h1>Settings</h1>
        <p>Manage API keys and account settings.</p>
      </div>

      <div class="settings-section">
        <h2>API Keys</h2>
        <p class="text-muted" style="font-size:13px; margin-bottom:16px;">
          Keys are stored in your OS keyring (macOS Keychain / Windows Credential Manager / Linux Secret Service).
          They are never sent to the frontend or logged.
        </p>
        ${keyRows}
      </div>

      <div class="settings-section">
        <h2>About</h2>
        <p style="font-size:13px; color:var(--text-secondary);">
          Audire &mdash; local-first meeting transcription.<br/>
          Privacy-first: no audio written to disk, BYOK keys, encrypted DB.
        </p>
      </div>
    </div>
  `;

  // Save key handlers
  document.querySelectorAll('[data-save-key]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.saveKey;
      const input = document.getElementById(`key-${provider}`);
      const key = input?.value.trim();
      if (!key) return;
      try {
        await invoke('save_api_key', { provider, key });
        input.value = '';
        renderSettings(); // re-render to update status
      } catch (e) {
        alert(`Failed to save key: ${e}`);
      }
    });
  });

  // Delete key handlers
  document.querySelectorAll('[data-delete-key]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.deleteKey;
      try {
        await invoke('delete_api_key', { provider });
        renderSettings();
      } catch (e) {
        alert(`Failed to delete key: ${e}`);
      }
    });
  });
}

async function renderNoteEditor() {
  if (!currentStandaloneNoteId) {
    navigate('notes');
    return;
  }

  let note;
  try {
    note = await invoke('get_standalone_note', { noteId: currentStandaloneNoteId });
  } catch (e) {
    console.error('get_standalone_note error:', e);
    navigate('notes');
    return;
  }

  content.innerHTML = `
    <div class="view">
      <div style="display:flex; align-items:center; gap:12px; margin-bottom:8px;">
        <button class="btn btn-ghost" id="noteBackBtn">Back</button>
        <button class="btn-danger-sm" id="noteDeleteBtn">Delete</button>
      </div>
      <input class="note-title-input" id="noteTitleInput" value="${escapeHtml(note.title)}" placeholder="Note title..." />
      <div class="notes-editor" contenteditable="true" id="noteBodyEditor">${escapeHtml(note.text)}</div>
    </div>
  `;

  const titleInput = document.getElementById('noteTitleInput');
  const bodyEditor = document.getElementById('noteBodyEditor');

  async function saveNote() {
    const title = titleInput?.value.trim() || 'Untitled';
    const text = bodyEditor?.innerText || '';
    try {
      await invoke('update_standalone_note', {
        noteId: currentStandaloneNoteId,
        title,
        text,
      });
    } catch (e) {
      console.error('save note error:', e);
    }
  }

  titleInput?.addEventListener('blur', saveNote);
  bodyEditor?.addEventListener('blur', saveNote);

  document.getElementById('noteBackBtn')?.addEventListener('click', () => {
    saveNote();
    navigate('notes');
  });

  document.getElementById('noteDeleteBtn')?.addEventListener('click', async () => {
    try {
      await invoke('delete_standalone_note', { noteId: currentStandaloneNoteId });
    } catch (e) {
      console.error('delete note error:', e);
    }
    currentStandaloneNoteId = null;
    navigate('notes');
  });
}

function renderRecipes() {
  content.innerHTML = `
    <div class="view">
      <div class="view-header">
        <h1>Recipes</h1>
        <p>Create, share and browse really good prompts.</p>
      </div>

      <div class="tab-bar mb-16">
        <button class="tab active">Discover</button>
        <button class="tab">My recipes</button>
      </div>

      <h3 style="font-size: 14px; font-weight: 600; margin-bottom: 12px;">Built-in recipes</h3>
      <div class="recipes-grid">
        <div class="recipe-card" id="recipeSummary">
          <h4>Summary</h4>
          <p>Generate a concise summary of your meeting transcript combined with your notes.</p>
        </div>
        <div class="recipe-card">
          <h4>Action items</h4>
          <p>Extract action items and next steps from the meeting. (Requires LLM feature flag)</p>
        </div>
        <div class="recipe-card">
          <h4>Follow-up email</h4>
          <p>Draft a follow-up email based on meeting decisions. (Requires LLM feature flag)</p>
        </div>
        <div class="recipe-card">
          <h4>Key decisions</h4>
          <p>List the key decisions made during the meeting using FTS5 retrieval.</p>
        </div>
      </div>

      <h3 style="font-size: 14px; font-weight: 600; margin: 24px 0 12px;">Across meetings</h3>
      <div class="recipes-grid">
        <div class="recipe-card">
          <h4>List recent todos</h4>
          <p>Extracts outstanding to-dos from recent meeting notes.</p>
        </div>
        <div class="recipe-card">
          <h4>Weekly recap</h4>
          <p>Generates a status overview of the past week's meetings.</p>
        </div>
      </div>
    </div>
  `;

  document.getElementById('recipeSummary')?.addEventListener('click', async () => {
    if (!meetingId) {
      alert('Start a capture session first.');
      return;
    }
    navigate('meeting');
    try {
      const out = await invoke('run_recipe', { meetingId, recipeId: 'summary' });
      const editor = document.getElementById('notesEditor');
      if (editor) editor.innerHTML = `<pre style="white-space: pre-wrap;">${escapeHtml(out.text)}</pre>`;
    } catch (e) { console.error(e); }
  });
}

// ---- Helpers ----
function updateTranscript() {
  const el = document.getElementById('transcriptContent');
  if (!el) return;

  let html = '';
  for (const f of finals) {
    const ts = new Date(f.ts_ms);
    const timeStr = ts.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    html += `<div class="transcript-line final"><span class="ts">${timeStr}</span> ${escapeHtml(f.text)}</div>`;
  }
  if (partialText) {
    html += `<div class="transcript-line partial">${escapeHtml(partialText)}</div>`;
  }
  el.innerHTML = html;

  // Auto-scroll
  const body = document.getElementById('transcriptBody');
  if (body) body.scrollTop = body.scrollHeight;
}

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str;
  return d.innerHTML;
}

// ---- Init ----
const initHash = location.hash.slice(1) || 'home';
navigate(initHash, false);
