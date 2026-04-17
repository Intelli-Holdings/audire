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
let meetingSegments = [];
let currentStructuredNote = null;
let currentMeeting = null;
let meetingUserNotes = [];
let meetingTemplates = [];
let foldersCache = [];
let currentFolderId = null;
let captureStartedAt = null;
let captureTimerHandle = null;
let captureRuntimeStatusText = 'Listening';
let capturePreviewText = 'Waiting for speech...';
let selectedCaptureProvider = 'deepgram';

// ---- Elements ----
const content = document.getElementById('content');
const searchModal = document.getElementById('searchModal');
const searchInput = document.getElementById('searchInput');
const captureBar = document.getElementById('captureBar');
const captureStatus = document.getElementById('captureStatus');
const captureRuntimeStatus = document.getElementById('captureRuntimeStatus');
const capturePreview = document.getElementById('capturePreview');
const captureTimer = document.getElementById('captureTimer');
const captureStopBtn = document.getElementById('captureStopBtn');

// ---- Event listeners from Tauri ----
async function registerTauriListeners() {
  await listen('asr:partial', (event) => {
    partialText = event.payload?.text || '';
    if (partialText) {
      capturePreviewText = partialText;
      updateCapturePillText();
    }
    updateTranscript();
  });

  await listen('asr:final', (event) => {
    const t = event.payload?.text || '';
    const ts = event.payload?.ts_ms || Date.now();
    const prov = event.payload?.provider || '';
    if (t) {
      finals.push({ text: t, ts_ms: ts, provider: prov });
      partialText = '';
      capturePreviewText = t;
      updateCapturePillText();
      updateTranscript();
    }
  });

  await listen('asr:status', (event) => {
    const s = event.payload?.status || '';
    if (s) {
      captureRuntimeStatusText = s;
      updateCapturePillText();
      if (!isCapturing && captureStatus) {
        captureStatus.textContent = s;
      }
    }
  });

  await listen('asr:lifecycle', (event) => {
    const nextState = event.payload?.state || '';
    const eventMeetingId = event.payload?.meeting_id || '';
    const message = event.payload?.message || '';

    if (eventMeetingId && meetingId && eventMeetingId !== meetingId) return;

    if (nextState === 'running') {
      captureRuntimeStatusText = 'Live';
      updateCapturePillText();
      return;
    }

    if (nextState === 'stopped') {
      isCapturing = false;
      hideCapturePill();
      if (currentView === 'meeting') {
        renderMeeting();
      }
      return;
    }

    if (nextState === 'error') {
      isCapturing = false;
      hideCapturePill();
      if (message) {
        showToast(message, 'error');
      }
      if (currentView === 'meeting') {
        renderMeeting();
      }
    }
  });
}

void registerTauriListeners();

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

function formatCaptureDuration(msElapsed) {
  const totalSeconds = Math.max(0, Math.floor(msElapsed / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${String(hours).padStart(2, '0')}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
  }
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
}

function updateCaptureTimer() {
  if (!captureTimer) return;
  if (!captureStartedAt) {
    captureTimer.textContent = '00:00';
    return;
  }
  captureTimer.textContent = formatCaptureDuration(Date.now() - captureStartedAt);
}

function updateCapturePillText() {
  if (captureRuntimeStatus) {
    captureRuntimeStatus.textContent = captureRuntimeStatusText || 'Listening';
  }
  if (capturePreview) {
    capturePreview.textContent = capturePreviewText || 'Waiting for speech...';
  }
}

function showCapturePill(provider) {
  captureStartedAt = Date.now();
  captureRuntimeStatusText = 'Connecting';
  capturePreviewText = 'Waiting for speech...';
  captureBar.classList.remove('hidden');
  captureStatus.textContent = `Recording ${provider}`;
  updateCapturePillText();
  updateCaptureTimer();
  if (captureTimerHandle) clearInterval(captureTimerHandle);
  captureTimerHandle = window.setInterval(updateCaptureTimer, 1000);
}

function hideCapturePill() {
  if (captureTimerHandle) {
    clearInterval(captureTimerHandle);
    captureTimerHandle = null;
  }
  captureStartedAt = null;
  captureRuntimeStatusText = 'Listening';
  capturePreviewText = 'Waiting for speech...';
  updateCaptureTimer();
  updateCapturePillText();
  captureBar.classList.add('hidden');
  captureStatus.textContent = 'Idle';
}

function showToast(message, kind = 'info') {
  const toast = document.createElement('div');
  toast.className = `app-toast ${kind}`;
  toast.textContent = message;
  document.body.appendChild(toast);
  window.requestAnimationFrame(() => toast.classList.add('visible'));
  window.setTimeout(() => {
    toast.classList.remove('visible');
    window.setTimeout(() => toast.remove(), 220);
  }, 2600);
}

async function chooseDefaultProvider() {
  const providers = ['deepgram', 'assemblyai'];
  for (const provider of providers) {
    try {
      if (await invoke('has_api_key', { provider })) {
        return provider;
      }
    } catch {
      continue;
    }
  }
  return 'mock';
}

async function startCapture(provider, opts = {}) {
  if (isCapturing) return;
  finals.length = 0;
  partialText = '';
  capturePreviewText = 'Waiting for speech...';

  try {
    const resp = await invoke('start_capture', {
      provider,
      mode: opts.mode || 'system',
      includeMic: opts.includeMic !== false,
      targetProcess: opts.targetProcess || null,
    });
    meetingId = resp.meeting_id;
    isCapturing = true;
    showCapturePill(provider);
    showToast(`Recording started with ${provider}`, 'success');
    navigate('meeting');
  } catch (e) {
    console.error(e);
    showToast(`Failed to start: ${e}`, 'error');
  }
}

async function stopCapture() {
  if (!isCapturing || !meetingId) return;
  try {
    await invoke('stop_capture', { meetingId });
  } catch (e) {
    console.error(e);
    showToast(`Stop failed: ${e}`, 'error');
  } finally {
    isCapturing = false;
    hideCapturePill();
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
  const providerCards = [
    { id: 'deepgram', label: 'Deepgram', desc: 'Fast live transcription' },
    { id: 'assemblyai', label: 'AssemblyAI', desc: 'Alternative live ASR' },
    { id: 'mock', label: 'Mock', desc: 'Offline UI testing' },
  ];

  // Fetch recent meetings
  let meetings = [];
  try {
    meetings = await invoke('list_meetings');
  } catch (e) {
    console.error('list_meetings error:', e);
  }

  if (!selectedCaptureProvider) {
    selectedCaptureProvider = await chooseDefaultProvider();
  }

  for (const provider of providerCards) {
    if (provider.id === 'mock') {
      keyStatusCache.mock = true;
      continue;
    }
    try {
      keyStatusCache[provider.id] = await invoke('has_api_key', { provider: provider.id });
    } catch {
      keyStatusCache[provider.id] = false;
    }
  }

  if (!keyStatusCache[selectedCaptureProvider] && selectedCaptureProvider !== 'mock') {
    selectedCaptureProvider = await chooseDefaultProvider();
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
        <div class="capture-hero">
          <div>
            <h3>Start a meeting</h3>
            <p class="capture-subtitle">Local-first capture with structured notes, live transcript, and evidence-linked memory.</p>
          </div>
          <div class="capture-hero-actions">
            <button class="btn btn-primary" id="startBtn" ${isCapturing ? 'disabled' : ''}>
              ${isCapturing ? 'Recording...' : 'Start now'}
            </button>
            ${isCapturing ? `<button class="btn btn-ghost" id="viewMeetingBtn">Open live view</button>` : ''}
          </div>
        </div>
        <div class="capture-provider-grid">
          ${providerCards.map(provider => `
            <button
              class="capture-provider-card ${provider.id === selectedCaptureProvider ? 'active' : ''} ${!keyStatusCache[provider.id] ? 'inactive' : ''}"
              data-provider-card="${provider.id}"
              type="button"
            >
              <span class="capture-provider-title">
                ${escapeHtml(provider.label)}
                ${keyStatusCache[provider.id] ? '<span class="capture-provider-dot ready"></span>' : '<span class="capture-provider-dot"></span>'}
              </span>
              <span class="capture-provider-desc">${escapeHtml(provider.desc)}</span>
            </button>
          `).join('')}
        </div>
        <div class="capture-options compact">
          <label class="toggle-label toggle-chip">
            <input type="checkbox" id="includeMicToggle" checked />
            <span>Mic on</span>
          </label>
          <div class="capture-mode-pill">System audio loopback</div>
          <a class="capture-inline-link" href="#settings">${selectedCaptureProvider !== 'mock' && !keyStatusCache[selectedCaptureProvider] ? 'Add API key to use this provider' : 'Manage keys'}</a>
        </div>
        <div class="status-line mt-16 ${selectedCaptureProvider !== 'mock' && !keyStatusCache[selectedCaptureProvider] ? 'error' : 'success'}">
          ${selectedCaptureProvider === 'mock'
            ? 'Mock provider selected for local UI testing.'
            : keyStatusCache[selectedCaptureProvider]
              ? `${providerCards.find(p => p.id === selectedCaptureProvider)?.label || 'Provider'} is configured in your OS keyring.`
              : `${providerCards.find(p => p.id === selectedCaptureProvider)?.label || 'Provider'} has no saved key yet.`}
        </div>
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
    const includeMic = document.getElementById('includeMicToggle')?.checked ?? true;
    startCapture(selectedCaptureProvider, { includeMic });
  });
  document.getElementById('viewMeetingBtn')?.addEventListener('click', () => navigate('meeting'));
  document.querySelectorAll('[data-provider-card]').forEach(el => {
    el.addEventListener('click', () => {
      selectedCaptureProvider = el.dataset.providerCard;
      renderHome();
    });
  });

  // Click on recent meeting items
  document.querySelectorAll('[data-mid]').forEach(el => {
    el.addEventListener('click', () => {
      meetingId = el.dataset.mid;
      navigate('meeting');
    });
  });
}

async function renderMeeting() {
  if (meetingTemplates.length === 0) {
    try {
      meetingTemplates = await invoke('list_meeting_templates');
    } catch (e) {
      console.error('Template load error:', e);
    }
  }
  if (foldersCache.length === 0) {
    try {
      foldersCache = await invoke('list_folders');
    } catch (e) {
      console.error('Folder load error:', e);
    }
  }

  meetingSegments = [];
  meetingUserNotes = [];
  currentStructuredNote = null;
  currentMeeting = null;

  if (meetingId) {
    try {
      const detail = await invoke('get_meeting_detail', { meetingId });
      currentMeeting = detail.meeting;
      meetingSegments = detail.segments || [];
      meetingUserNotes = detail.user_notes || [];
      currentStructuredNote = detail.structured_note || null;
    } catch (e) {
      console.error('Meeting detail load error:', e);
    }
  }

  const templateKind = currentMeeting?.template_kind || 'generic';
  const title = currentMeeting?.title || (meetingId ? 'Meeting Notes' : 'Live Session');
  const folderOptions = ['<option value="">No folder</option>'].concat(
    foldersCache.map(folder => `
      <option value="${folder.id}" ${currentMeeting?.folder_id === folder.id ? 'selected' : ''}>
        ${escapeHtml(folder.name)}
      </option>
    `),
  ).join('');
  const templateOptions = meetingTemplates.map(t => `
    <option value="${escapeHtml(t.kind)}" ${t.kind === templateKind ? 'selected' : ''}>
      ${escapeHtml(t.label)}
    </option>
  `).join('');

  const userNotesHtml = meetingUserNotes.length > 0
    ? meetingUserNotes.map(n => `
      <div class="user-note-row">
        <div class="user-note-meta">${formatDateTime(n.ts_ms)}</div>
        <div class="user-note-text">${escapeHtml(n.text)}</div>
      </div>
    `).join('')
    : '<p class="text-muted">No saved human notes yet.</p>';

  content.innerHTML = `
    <div class="meeting-detail" style="padding-bottom: 320px;">
      <div class="meeting-detail-header">
        <h1>${escapeHtml(title)}</h1>
        <div class="meeting-badges">
          <span class="badge">${escapeHtml((templateKind || 'generic').replaceAll('_', ' '))}</span>
          ${isCapturing ? '<span class="badge recording-badge">Recording</span>' : ''}
        </div>
        <div class="meeting-actions">
          <button class="btn btn-ghost" id="exportBtn">Export MD</button>
        </div>
      </div>

      <div class="settings-section meeting-template-bar">
        <div>
          <h2>Meeting template</h2>
          <p class="text-muted" style="font-size:13px;">Templates steer the structure and retrieval emphasis.</p>
        </div>
        <div class="meeting-template-actions">
          <div class="select-wrap">
            <select id="meetingFolderSelect">${folderOptions}</select>
          </div>
          <div class="select-wrap">
            <select id="meetingTemplateSelect">${templateOptions}</select>
          </div>
          <button class="btn-primary-sm" id="generateStructuredBtn">Generate structured notes</button>
        </div>
      </div>

      <div class="meeting-grid">
        <div class="settings-section">
          <h2>Human notes</h2>
          <textarea class="chat-textarea" id="meetingNoteInput" placeholder="Capture your own outline, decisions, actions, risks, or questions..." rows="4"></textarea>
          <div style="display:flex; justify-content:flex-end; margin-top:10px;">
            <button class="btn-primary-sm" id="saveMeetingNoteBtn">Save note</button>
          </div>
          <div class="user-notes-list mt-16">${userNotesHtml}</div>
        </div>

        <div class="settings-section">
          <div class="structured-notes-header">
            <div>
              <h2>Structured notes</h2>
              <p class="text-muted" style="font-size:13px;">Editable notes grounded in transcript evidence.</p>
            </div>
            ${currentStructuredNote ? `<span class="key-status configured">Ready</span>` : `<span class="key-status missing">Not generated</span>`}
          </div>
          <div id="structuredNotesRoot">
            ${renderStructuredNotes(currentStructuredNote)}
          </div>
        </div>
      </div>

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
          <h3>Transcript (${meetingSegments.length} segments)</h3>
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
          Refresh notes
        </button>
        <button class="recipe-chip" onclick="window.location.hash='recipes'">
          <span class="chip-icon">/</span>
          See all
        </button>
      </div>
    </div>
  `;

  document.getElementById('transcriptToggle')?.addEventListener('click', () => {
    document.getElementById('transcriptPanel')?.classList.toggle('collapsed');
  });

  document.getElementById('exportBtn')?.addEventListener('click', exportMeeting);

  document.getElementById('meetingTemplateSelect')?.addEventListener('change', async (e) => {
    if (!meetingId) return;
    const nextTemplate = e.target.value;
    try {
      await invoke('set_meeting_template', { meetingId, templateKind: nextTemplate });
      if (currentMeeting) currentMeeting.template_kind = nextTemplate;
    } catch (err) {
      console.error('Template update error:', err);
    }
  });

  document.getElementById('meetingFolderSelect')?.addEventListener('change', async (e) => {
    if (!meetingId) return;
    const nextFolderId = e.target.value ? parseInt(e.target.value) : null;
    try {
      await invoke('assign_meeting_folder', { meetingId, folderId: nextFolderId });
      if (currentMeeting) currentMeeting.folder_id = nextFolderId;
    } catch (err) {
      console.error('Meeting folder update error:', err);
    }
  });

  document.getElementById('saveMeetingNoteBtn')?.addEventListener('click', async () => {
    if (!meetingId) return;
    const input = document.getElementById('meetingNoteInput');
    const text = input?.value.trim();
    if (!text) return;
    try {
      await invoke('append_note', { meetingId, text });
      input.value = '';
      renderMeeting();
    } catch (e) {
      console.error('Note save error:', e);
    }
  });

  document.getElementById('generateStructuredBtn')?.addEventListener('click', async () => {
    if (!meetingId) return;
    try {
      currentStructuredNote = await invoke('generate_structured_meeting_notes', {
        meetingId,
        templateKind: document.getElementById('meetingTemplateSelect')?.value || templateKind,
      });
      renderMeeting();
    } catch (e) {
      console.error('Structured note generation error:', e);
    }
  });

  document.getElementById('runSummaryBtn')?.addEventListener('click', async () => {
    document.getElementById('generateStructuredBtn')?.click();
  });

  bindStructuredNoteEditors();
  updateTranscript();
}

function renderStructuredNotes(note) {
  if (!note) {
    return `
      <div class="empty-state" style="padding: 24px 12px;">
        <h3>Structured notes are empty</h3>
        <p>Generate notes to capture summary, decisions, actions, questions, risks, and evidence-linked highlights.</p>
      </div>
    `;
  }

  const sectionsHtml = note.sections.map(section => `
    <div class="structured-section">
      <h3>${escapeHtml(section.label)}</h3>
      ${section.items.length > 0 ? section.items.map(item => `
        <div class="structured-item">
          <textarea class="structured-item-input" data-structured-item-id="${item.id}" rows="2">${escapeHtml(item.text)}</textarea>
          <div class="structured-item-meta">
            <span class="badge subtle">${escapeHtml(item.author_kind)}</span>
            <span class="text-muted">${item.evidence_count} evidence source${item.evidence_count === 1 ? '' : 's'}</span>
            ${item.retrieval_confidence ? `<span class="text-muted">confidence ${(item.retrieval_confidence * 100).toFixed(0)}%</span>` : ''}
          </div>
          <div class="citation-row">
            ${item.citations.map(citation => `
              <button class="citation-chip" data-segment-jump="${citation.segment_id}">
                ${escapeHtml(formatShortTime(citation.ts_ms))} · ${escapeHtml(citation.excerpt)}
              </button>
            `).join('')}
          </div>
        </div>
      `).join('') : '<p class="text-muted">No items yet.</p>'}
    </div>
  `).join('');

  return `
    <div class="structured-summary-card">
      <label class="text-muted" style="font-size:12px;">Summary</label>
      <textarea id="structuredSummaryInput" class="chat-textarea" rows="4">${escapeHtml(note.summary || '')}</textarea>
    </div>
    <div class="structured-sections">${sectionsHtml}</div>
  `;
}

function bindStructuredNoteEditors() {
  document.getElementById('structuredSummaryInput')?.addEventListener('blur', async (e) => {
    if (!meetingId || !currentStructuredNote) return;
    const summary = e.target.value.trim();
    try {
      await invoke('update_structured_note_summary', { meetingId, summary });
      currentStructuredNote.summary = summary;
    } catch (err) {
      console.error('Structured summary update error:', err);
    }
  });

  document.querySelectorAll('[data-structured-item-id]').forEach(el => {
    el.addEventListener('blur', async () => {
      const itemId = parseInt(el.dataset.structuredItemId);
      const text = el.value.trim();
      if (!text) return;
      try {
        await invoke('update_structured_note_item', { itemId, text });
      } catch (err) {
        console.error('Structured item update error:', err);
      }
    });
  });

  document.querySelectorAll('[data-segment-jump]').forEach(el => {
    el.addEventListener('click', () => {
      jumpToTranscriptSegment(parseInt(el.dataset.segmentJump));
    });
  });
}

async function renderChat() {
  if (foldersCache.length === 0) {
    try {
      foldersCache = await invoke('list_folders');
    } catch (e) {
      console.error('Folder load error:', e);
    }
  }

  content.innerHTML = `
    <div class="view">
      <div class="view-header">
        <h1>Ask anything</h1>
      </div>

      <div class="chat-input-area">
        <div class="chat-input-header">
          <span class="scope-tag active" data-scope="meeting">This meeting</span>
          <span class="scope-tag" data-scope="folder">Folder</span>
          <span class="scope-tag" data-scope="all">All meetings</span>
        </div>
        <div class="inline-form" style="margin-top:0; padding-top:0;">
          <select id="askFolderSelect" style="min-width:200px;">
            <option value="">All folders</option>
            ${foldersCache.map(folder => `<option value="${folder.id}">${escapeHtml(folder.name)}</option>`).join('')}
          </select>
        </div>
        <textarea class="chat-textarea" id="chatTextarea" placeholder="What decisions were made?" rows="2"></textarea>
        <div class="chat-input-footer">
          <div class="model-selector">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/></svg>
            Grounded local retrieval
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
    const text = textarea.value.trim();
    if (!text) return;
    const scope = document.querySelector('.scope-tag.active')?.dataset.scope || 'all';
    const folderId = document.getElementById('askFolderSelect')?.value
      ? parseInt(document.getElementById('askFolderSelect').value)
      : null;
    try {
      const out = await invoke('ask_audire', { query: text, scope, meetingId, folderId });
      document.getElementById('chatResponse').innerHTML =
        `<div class="settings-section">
          <pre style="white-space:pre-wrap;font-size:13px;color:var(--text-primary);">${escapeHtml(out.answer)}</pre>
          <div class="citation-row" style="margin-top:14px;">
            ${out.citations.map(citation => `
              <button class="citation-chip" data-chat-segment-id="${citation.segment_id || ''}" data-chat-meeting-id="${citation.meeting_id || ''}">
                ${escapeHtml(citation.title)}${citation.ts_ms ? ` · ${escapeHtml(formatShortTime(citation.ts_ms))}` : ''} · ${escapeHtml(citation.excerpt.slice(0, 90))}
              </button>
            `).join('')}
          </div>
        </div>`;
      document.querySelectorAll('[data-chat-segment-id]').forEach(el => {
        el.addEventListener('click', () => {
          const segmentId = parseInt(el.dataset.chatSegmentId);
          const nextMeetingId = el.dataset.chatMeetingId || null;
          if (!Number.isNaN(segmentId)) {
            if (nextMeetingId) meetingId = nextMeetingId;
            navigate('meeting');
            window.setTimeout(() => jumpToTranscriptSegment(segmentId), 120);
          }
        });
      });
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
  let folders = [];
  let selectedFolder = null;
  try {
    [meetings, standaloneNotes, folders] = await Promise.all([
      invoke('list_meetings'),
      invoke('list_standalone_notes'),
      invoke('list_folders'),
    ]);
    foldersCache = folders;
    if (currentFolderId) {
      selectedFolder = await invoke('get_folder_detail', { folderId: currentFolderId });
    }
  } catch (e) {
    console.error('Notes load error:', e);
  }

  let html = '<div class="view"><div class="view-header"><h1>My notes</h1></div>';

  if (folders.length > 0) {
    html += '<div class="notes-sub-header"><h2>Folders</h2></div><div class="notes-list">';
    for (const folder of folders) {
      html += `
        <div class="note-card" data-folder-id="${folder.id}">
          <div class="note-card-title">${escapeHtml(folder.name)}</div>
          <div class="note-card-meta">${escapeHtml(folder.kind)} &middot; ${folder.meeting_count} meeting${folder.meeting_count !== 1 ? 's' : ''}</div>
          <div class="note-card-preview">${folder.note_count} standalone note${folder.note_count !== 1 ? 's' : ''}</div>
        </div>`;
    }
    html += '</div>';
  }

  html += `
    <div class="inline-form" id="addFolderForm">
      <input type="text" id="folderName" placeholder="Folder name" style="flex:1;" />
      <select id="folderKind">
        <option value="project">Project</option>
        <option value="client">Client</option>
        <option value="team">Team</option>
        <option value="topic">Topic</option>
      </select>
      <button class="btn-primary-sm" id="addFolderBtn">Create folder</button>
    </div>
  `;

  if (selectedFolder) {
    html += `
      <div class="settings-section">
        <h2>${escapeHtml(selectedFolder.folder.name)}</h2>
        <p class="text-muted" style="font-size:13px;">${escapeHtml(selectedFolder.folder.kind)} folder</p>
        <div class="notes-sub-header" style="margin-top:16px;"><h2>Meetings</h2></div>
        <div class="notes-list">
          ${selectedFolder.meetings.length > 0 ? selectedFolder.meetings.map(m => `
            <div class="note-card" data-meeting-note-id="${escapeHtml(m.id)}">
              <div class="note-card-title">${escapeHtml(m.title || `${m.provider} session`)}</div>
              <div class="note-card-meta">${m.note_count} note${m.note_count !== 1 ? 's' : ''}</div>
              <div class="note-card-preview">${escapeHtml((m.note_preview || '').slice(0, 100))}</div>
            </div>
          `).join('') : '<p class="text-muted">No meetings in this folder yet.</p>'}
        </div>
        <div class="notes-sub-header" style="margin-top:20px;"><h2>Standalone Notes</h2></div>
        <div class="notes-list">
          ${selectedFolder.standalone_notes.length > 0 ? selectedFolder.standalone_notes.map(n => `
            <div class="note-card" data-standalone-id="${n.id}">
              <div class="note-card-title">${escapeHtml(n.title)}</div>
              <div class="note-card-preview">${escapeHtml(n.text.slice(0, 100))}</div>
            </div>
          `).join('') : '<p class="text-muted">No standalone notes in this folder yet.</p>'}
        </div>
      </div>
    `;
  }

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
          <div class="note-card-meta">${dateStr}${n.folder_name ? ` &middot; ${escapeHtml(n.folder_name)}` : ''}</div>
          <div class="note-card-preview">${escapeHtml(n.text.slice(0, 120))}</div>
        </div>`;
    }
    html += '</div>';
  }

  // Meeting notes section
  const meetingsWithNotes = meetings.filter(m => m.note_count > 0 || m.has_structured_notes);
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
          <div class="note-card-meta">${dateStr} &middot; ${m.note_count} note${m.note_count !== 1 ? 's' : ''}${m.folder_name ? ` &middot; ${escapeHtml(m.folder_name)}` : ''}</div>
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

  document.querySelectorAll('[data-folder-id]').forEach(el => {
    el.addEventListener('click', () => {
      currentFolderId = parseInt(el.dataset.folderId);
      renderNotes();
    });
  });

  document.getElementById('addFolderBtn')?.addEventListener('click', async () => {
    const name = document.getElementById('folderName')?.value.trim();
    const kind = document.getElementById('folderKind')?.value || 'project';
    if (!name) return;
    try {
      await invoke('create_folder', { name, kind, color: null });
      renderNotes();
    } catch (e) {
      console.error('Create folder error:', e);
    }
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
  let orgKeyCounts = {};
  try {
    orgs = await invoke('list_organizations');
    const statusLists = await Promise.all(orgs.map(async org => {
      try {
        const statuses = await invoke('list_org_shared_key_statuses', { orgId: org.id });
        return [org.id, statuses.length];
      } catch {
        return [org.id, 0];
      }
    }));
    orgKeyCounts = Object.fromEntries(statusLists);
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
              <div class="person-email">Shared keys: ${orgKeyCounts[o.id] || 0}</div>
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
    let source = 'Unavailable';
    try {
      const resolution = await invoke('resolve_provider_key_source', { provider: p.id, orgId: null });
      source = resolution?.source ? resolution.source.replaceAll('_', ' ') : source;
    } catch {
      source = has ? 'Available' : 'Unavailable';
    }
    keyRows += `
      <div class="key-row">
        <span class="provider-label">${p.label}</span>
        <span class="key-status ${has ? 'configured' : 'missing'}">${has ? 'Configured' : 'Not set'}</span>
        <input type="password" class="key-input" id="key-${p.id}" placeholder="${p.desc}" />
        <span class="text-muted" style="font-size:12px; min-width:100px;">${escapeHtml(source)}</span>
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
        showToast(`${provider} key saved`, 'success');
        renderSettings(); // re-render to update status
      } catch (e) {
        showToast(`Failed to save key: ${e}`, 'error');
      }
    });
  });

  document.querySelectorAll('.key-input').forEach(input => {
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') {
        event.preventDefault();
        input.closest('.key-row')?.querySelector('[data-save-key]')?.click();
      }
    });
  });

  // Delete key handlers
  document.querySelectorAll('[data-delete-key]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.deleteKey;
      try {
        await invoke('delete_api_key', { provider });
        showToast(`${provider} key deleted`, 'success');
        renderSettings();
      } catch (e) {
        showToast(`Failed to delete key: ${e}`, 'error');
      }
    });
  });
}

async function renderNoteEditor() {
  if (!currentStandaloneNoteId) {
    navigate('notes');
    return;
  }

  if (foldersCache.length === 0) {
    try {
      foldersCache = await invoke('list_folders');
    } catch (e) {
      console.error('Folder load error:', e);
    }
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
      <div class="inline-form" style="margin-top:0; margin-bottom:12px;">
        <select id="noteFolderSelect" style="min-width:180px;">
          <option value="">No folder</option>
          ${foldersCache.map(folder => `
            <option value="${folder.id}" ${note.folder_id === folder.id ? 'selected' : ''}>
              ${escapeHtml(folder.name)}
            </option>
          `).join('')}
        </select>
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
  document.getElementById('noteFolderSelect')?.addEventListener('change', async (e) => {
    try {
      await invoke('assign_standalone_note_folder', {
        noteId: currentStandaloneNoteId,
        folderId: e.target.value ? parseInt(e.target.value) : null,
      });
    } catch (err) {
      console.error('Assign note folder error:', err);
    }
  });

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
      await invoke('generate_structured_meeting_notes', { meetingId, templateKind: currentMeeting?.template_kind || 'generic' });
      renderMeeting();
    } catch (e) { console.error(e); }
  });
}

// ---- Helpers ----
function updateTranscript() {
  const el = document.getElementById('transcriptContent');
  if (!el) return;

  let html = '';
  const transcriptRows = meetingSegments.length > 0 && !isCapturing
    ? meetingSegments.map(seg => ({
        id: seg.id,
        ts_ms: seg.ts_ms,
        text: seg.text,
        kind: 'final',
      }))
    : finals.map(f => ({
        id: null,
        ts_ms: f.ts_ms,
        text: f.text,
        kind: 'final',
      }));

  for (const row of transcriptRows) {
    html += `
      <div class="transcript-line ${row.kind}" ${row.id ? `data-transcript-segment="${row.id}"` : ''}>
        <span class="ts">${escapeHtml(formatDateTime(row.ts_ms))}</span>
        ${escapeHtml(row.text)}
      </div>`;
  }
  if (partialText) {
    html += `<div class="transcript-line partial">${escapeHtml(partialText)}</div>`;
  }
  el.innerHTML = html;

  // Auto-scroll
  const body = document.getElementById('transcriptBody');
  if (body) body.scrollTop = body.scrollHeight;
}

function jumpToTranscriptSegment(segmentId) {
  const line = document.querySelector(`[data-transcript-segment="${segmentId}"]`);
  if (!line) return;
  line.scrollIntoView({ behavior: 'smooth', block: 'center' });
  line.classList.add('flash-segment');
  window.setTimeout(() => line.classList.remove('flash-segment'), 1400);
}

function formatDateTime(tsMs) {
  return new Date(tsMs).toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function formatShortTime(tsMs) {
  const totalSeconds = Math.floor(tsMs / 1000);
  const minutes = Math.floor(totalSeconds / 60) % 60;
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
}

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

// ---- Init ----
const initHash = location.hash.slice(1) || 'home';
navigate(initHash, false);
