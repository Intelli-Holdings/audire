import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { getCurrentWindow, LogicalPosition, primaryMonitor } from '@tauri-apps/api/window';
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
let partialTranscript = null;
let captureValidated = false;
let selectedCaptureProvider = 'assemblyai';
const appWindow = getCurrentWindow();
const currentWindowLabel = appWindow.label;
const CAPTURE_UI_STATE_KEY = 'audire.capture.ui';
const SIDEBAR_COLLAPSED_KEY = 'audire.sidebar.collapsed';
let recorderPillMonitorHandle = null;

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
const sidebar = document.getElementById('sidebar');
const sidebarCollapseBtn = document.getElementById('sidebarCollapseBtn');
const sidebarFolders = document.getElementById('sidebarFolders');
const sidebarAddFolderBtn = document.getElementById('sidebarAddFolderBtn');
const folderModalOverlay = document.getElementById('folderModalOverlay');
const folderModalTitle = document.getElementById('folderModalTitle');
const folderModalDescription = document.getElementById('folderModalDescription');
const folderModalCreateBtn = document.getElementById('folderModalCreateBtn');
const folderModalCloseBtn = document.getElementById('folderModalCloseBtn');
const folderModalCancelBtn = document.getElementById('folderModalCancelBtn');

// ---- Event listeners from Tauri ----
async function registerTauriListeners() {
  await listen('asr:partial', (event) => {
    partialText = event.payload?.text || '';
    partialTranscript = partialText ? {
      text: partialText,
      formatted: Boolean(event.payload?.formatted),
    } : null;
    if (partialText) {
      capturePreviewText = partialText;
      updateCapturePillText();
    }
    updateTranscript();
    persistCaptureUiState();
  });

  await listen('asr:final', (event) => {
    const t = event.payload?.text || '';
    const ts = event.payload?.ts_ms || Date.now();
    const prov = event.payload?.provider || '';
    const formatted = Boolean(event.payload?.formatted);
    if (t) {
      finals.push({ text: t, ts_ms: ts, provider: prov, formatted });
      partialText = '';
      partialTranscript = null;
      capturePreviewText = t;
      updateCapturePillText();
      updateTranscript();
    }
    persistCaptureUiState();
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
      captureValidated = true;
      captureRuntimeStatusText = 'Live';
      updateCapturePillText();
      persistCaptureUiState();
      return;
    }

    if (nextState === 'stopped') {
      isCapturing = false;
      hideCapturePill();
      persistCaptureUiState();
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
      persistCaptureUiState();
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
  if (e.key === 'Escape') {
    closeSearch();
    closeFolderModal();
  }
});
searchModal.addEventListener('click', (e) => {
  if (e.target === searchModal) closeSearch();
});
folderModalOverlay?.addEventListener('click', (e) => {
  if (e.target === folderModalOverlay) closeFolderModal();
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
  restoreFloatingCaptureBar();

  // Update sidebar active state
  document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'));
  const active = document.querySelector(`[data-view="${view}"]`);
  if (active) active.classList.add('active');

  renderView(view);
}

// ---- New click handlers ----
document.getElementById('userProfile')?.addEventListener('click', () => navigate('settings'));
sidebarCollapseBtn?.addEventListener('click', toggleSidebarCollapsed);
sidebarAddFolderBtn?.addEventListener('click', openFolderModal);
folderModalCloseBtn?.addEventListener('click', closeFolderModal);
folderModalCancelBtn?.addEventListener('click', closeFolderModal);
folderModalTitle?.addEventListener('input', updateFolderModalState);
folderModalCreateBtn?.addEventListener('click', createFolderFromModal);
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
  syncRecorderPillUi();
  persistCaptureUiState();
}

function restoreFloatingCaptureBar() {
  if (!captureBar) return;
  document.body.appendChild(captureBar);
  captureBar.classList.remove('inline');
}

function setSidebarCollapsed(collapsed) {
  if (!sidebar) return;
  sidebar.classList.toggle('collapsed', collapsed);
  document.documentElement.style.setProperty(
    '--sidebar-width',
    collapsed ? '72px' : '260px',
  );
  try {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_KEY, collapsed ? '1' : '0');
  } catch {}
}

function toggleSidebarCollapsed() {
  setSidebarCollapsed(!sidebar?.classList.contains('collapsed'));
}

function hydrateSidebarState() {
  try {
    setSidebarCollapsed(window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === '1');
  } catch {
    setSidebarCollapsed(false);
  }
}

function openFolderModal() {
  folderModalOverlay?.classList.remove('hidden');
  folderModalTitle.value = '';
  folderModalDescription.value = '';
  updateFolderModalState();
  folderModalTitle?.focus();
}

function closeFolderModal() {
  folderModalOverlay?.classList.add('hidden');
}

function updateFolderModalState() {
  if (!folderModalCreateBtn || !folderModalTitle) return;
  folderModalCreateBtn.disabled = !folderModalTitle.value.trim();
}

async function createFolderFromModal() {
  const name = folderModalTitle?.value.trim();
  if (!name) return;
  try {
    await invoke('create_folder', { name, kind: 'project', color: null });
    closeFolderModal();
    await loadSidebarFolders();
    if (currentView === 'notes') {
      renderNotes();
    }
  } catch (e) {
    console.error('Create folder error:', e);
    showToast(`Failed to create folder: ${e}`, 'error');
  }
}

async function loadSidebarFolders() {
  if (!sidebarFolders) return;
  try {
    foldersCache = await invoke('list_folders');
  } catch (e) {
    console.error('Sidebar folders load error:', e);
    return;
  }

  sidebarFolders.innerHTML = foldersCache.map(folder => `
    <button class="sidebar-folder-item" type="button" data-sidebar-folder-id="${folder.id}">
      <svg class="sidebar-folder-icon" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M10 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-8l-2-2Z"/></svg>
      <div class="sidebar-folder-meta">
        <div class="folder-name">${escapeHtml(folder.name)}</div>
      </div>
    </button>
  `).join('');

  document.querySelectorAll('[data-sidebar-folder-id]').forEach(el => {
    el.addEventListener('click', () => {
      currentFolderId = parseInt(el.dataset.sidebarFolderId);
      navigate('notes');
    });
  });
}

function dockCaptureBarInMeeting() {
  if (!captureBar) return;
  const dock = document.getElementById('captureBarDock');
  if (!dock) return;
  dock.appendChild(captureBar);
  captureBar.classList.add('inline');
}

function showCapturePill(provider) {
  captureStartedAt = Date.now();
  captureRuntimeStatusText = 'Connecting';
  capturePreviewText = 'Waiting for speech...';
  captureBar.classList.remove('hidden');
  captureStatus.textContent = 'Recording';
  updateCapturePillText();
  updateCaptureTimer();
  if (captureTimerHandle) clearInterval(captureTimerHandle);
  captureTimerHandle = window.setInterval(updateCaptureTimer, 1000);
  void syncRecorderPillWindow();
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
  void syncRecorderPillWindow();
}

function persistCaptureUiState() {
  try {
    window.localStorage.setItem(CAPTURE_UI_STATE_KEY, JSON.stringify({
      meetingId,
      isCapturing,
      captureStartedAt,
      captureRuntimeStatusText,
      capturePreviewText,
      finals,
      partialText,
      partialTranscript,
    }));
  } catch {}
}

function hydrateCaptureUiState() {
  try {
    const raw = window.localStorage.getItem(CAPTURE_UI_STATE_KEY);
    if (!raw) return;
    const state = JSON.parse(raw);
    meetingId = state.meetingId ?? meetingId;
    isCapturing = Boolean(state.isCapturing);
    captureStartedAt = state.captureStartedAt ?? captureStartedAt;
    captureRuntimeStatusText = state.captureRuntimeStatusText || captureRuntimeStatusText;
    capturePreviewText = state.capturePreviewText || capturePreviewText;
    partialText = state.partialText || '';
    partialTranscript = partialText ? {
      text: partialText,
      formatted: Boolean(state.partialTranscript?.formatted),
    } : null;
    finals.length = 0;
    for (const row of state.finals || []) finals.push(row);

    if (isCapturing && captureBar) {
      captureBar.classList.remove('hidden');
      // Validate the capture is still running: if no lifecycle "running" event
      // arrives within 2s, assume it's stale and reset.
      setTimeout(() => {
        if (isCapturing && !captureValidated) {
          isCapturing = false;
          hideCapturePill();
          persistCaptureUiState();
          // Re-render current view to clear stale "LIVE" indicators
          const hash = location.hash.slice(1) || 'home';
          navigate(hash, false);
        }
      }, 2000);
    }
  } catch {}
}

function syncRecorderPillUi() {
  const statusEl = document.getElementById('recorderPillStatus');
  const previewEl = document.getElementById('recorderPillPreview');
  const timerEl = document.getElementById('recorderPillTimer');
  if (statusEl) statusEl.textContent = captureRuntimeStatusText || 'Listening';
  if (previewEl) previewEl.textContent = capturePreviewText || 'Waiting for speech...';
  if (timerEl) {
    timerEl.textContent = captureStartedAt
      ? formatCaptureDuration(Date.now() - captureStartedAt)
      : '00:00';
  }
  document.body.classList.toggle('pill-recording', isCapturing);
}

async function ensureRecorderPillWindow() {
  let pill = await WebviewWindow.getByLabel('recorder-pill');
  if (pill) return pill;

  pill = new WebviewWindow('recorder-pill', {
    url: '/#recorder-pill',
    width: 92,
    height: 232,
    resizable: false,
    decorations: false,
    transparent: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    focus: false,
    visible: false,
  });

  await new Promise((resolve, reject) => {
    const cleanup = [];
    pill.once('tauri://created', () => {
      cleanup.forEach(fn => fn());
      resolve();
    }).then(fn => cleanup.push(fn));
    pill.once('tauri://error', (event) => {
      cleanup.forEach(fn => fn());
      reject(event.payload);
    }).then(fn => cleanup.push(fn));
  });

  return pill;
}

async function positionRecorderPillWindow(pill) {
  const monitor = await primaryMonitor();
  const workArea = monitor?.workArea;
  if (!workArea) return;
  const x = workArea.position.x + workArea.size.width - 112;
  const y = workArea.position.y + Math.round((workArea.size.height - 232) / 2);
  await pill.setPosition(new LogicalPosition(x, y));
}

async function syncRecorderPillWindow() {
  if (currentWindowLabel !== 'main') return;
  const pill = await WebviewWindow.getByLabel('recorder-pill');

  if (!isCapturing) {
    if (pill) await pill.hide().catch(() => {});
    return;
  }

  const minimized = await appWindow.isMinimized().catch(() => false);
  if (!minimized) {
    if (pill) await pill.hide().catch(() => {});
    return;
  }

  const nextPill = pill || await ensureRecorderPillWindow();
  await positionRecorderPillWindow(nextPill).catch(() => {});
  await nextPill.show().catch(() => {});
}

function startRecorderPillMonitor() {
  if (currentWindowLabel !== 'main' || recorderPillMonitorHandle) return;
  recorderPillMonitorHandle = window.setInterval(() => {
    void syncRecorderPillWindow();
  }, 900);
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
  const providers = ['assemblyai', 'deepgram'];
  for (const provider of providers) {
    try {
      if (await invoke('has_api_key', { provider })) {
        return provider;
      }
    } catch {
      continue;
    }
  }
  return 'assemblyai';
}

async function startCapture(provider, opts = {}) {
  if (isCapturing) return;
  finals.length = 0;
  partialText = '';
  partialTranscript = null;
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
    captureValidated = true;
    showCapturePill(provider);
    persistCaptureUiState();
    showToast('Recording started', 'success');
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
    persistCaptureUiState();
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
  document.body.classList.toggle('recorder-pill-window', view === 'recorder-pill');
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
    case 'recorder-pill': renderRecorderPill(); break;
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

  if (!selectedCaptureProvider) {
    selectedCaptureProvider = await chooseDefaultProvider();
  }

  for (const provider of ['deepgram', 'assemblyai']) {
    try {
      keyStatusCache[provider] = await invoke('has_api_key', { provider });
    } catch {
      keyStatusCache[provider] = false;
    }
  }

  if (!keyStatusCache[selectedCaptureProvider]) {
    selectedCaptureProvider = await chooseDefaultProvider();
  }
  const hasConfiguredCaptureProvider = Boolean(keyStatusCache.deepgram || keyStatusCache.assemblyai);

  const recentHtml = meetings.length > 0
    ? meetings.slice(0, 5).map(m => {
        const d = new Date(m.started_at * 1000);
        const timeStr = d.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' });
        const title = m.title || 'Meeting notes';
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
            <p class="capture-subtitle">Clean local meeting capture with transcripts, notes, and structured summaries after the conversation.</p>
          </div>
          <div class="capture-hero-actions">
            <button class="btn btn-primary" id="startBtn" ${(isCapturing || !hasConfiguredCaptureProvider) ? 'disabled' : ''}>
              ${isCapturing ? 'Recording...' : 'Start now'}
            </button>
            ${isCapturing ? `<button class="btn btn-ghost" id="viewMeetingBtn">Open live view</button>` : ''}
          </div>
        </div>
        <div class="capture-options compact clean-capture-options">
          <label class="toggle-label toggle-chip">
            <input type="checkbox" id="includeMicToggle" checked />
            <span>Mic on</span>
          </label>
          <div class="capture-mode-pill">System audio</div>
          <a class="capture-inline-link" href="#settings">Transcription settings</a>
        </div>
        <div class="status-line mt-16 ${hasConfiguredCaptureProvider ? 'success' : 'error'}">
          ${hasConfiguredCaptureProvider
            ? 'Audire is ready to transcribe when you start recording.'
            : 'Add a transcription API key in Settings before starting your first recording.'}
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

  // Click on recent meeting items
  document.querySelectorAll('[data-mid]').forEach(el => {
    el.addEventListener('click', () => {
      meetingId = el.dataset.mid;
      navigate('meeting');
    });
  });
}

async function renderMeeting() {
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

  const title = currentMeeting?.title || (meetingId ? 'Meeting Notes' : 'Live Session');

  content.innerHTML = `
    <div class="meeting-detail">
      <div class="meeting-detail-header">
        <h1>${escapeHtml(title)}</h1>
        ${isCapturing ? '<span class="meeting-live-indicator">Live</span>' : ''}
      </div>

      <div id="captureBarDock" class="capture-bar-dock ${isCapturing ? '' : 'hidden'}"></div>

      <div class="transcript-live-panel" id="transcriptPanel">
        <div class="transcript-body" id="transcriptBody">
          <div class="transcript-feed" id="transcriptContent"></div>
        </div>
        ${isCapturing ? `
        <aside class="transcript-legend-inline">
          <span><em class="legend-partial">Italic</em> = unfinalized</span>
          <span class="legend-muted-label">Gray = finalized</span>
          <span class="legend-final-label">Black = formatted</span>
        </aside>
        ` : ''}
      </div>
    </div>
  `;

  updateTranscript();
  dockCaptureBarInMeeting();
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

  let recentMeetings = [];
  try {
    recentMeetings = await invoke('list_meetings');
  } catch {}

  content.innerHTML = `
    <div class="chat-view">
      <h1 class="chat-greeting">Hi, ask anything</h1>

      <div class="chat-composer">
        <div class="chat-composer-top">
          <div class="chat-scope-row">
            <span class="scope-pill active" data-scope="meeting">My notes</span>
            <span class="scope-pill" data-scope="all">All meetings</span>
            <select class="scope-folder-select" id="askFolderSelect">
              <option value="">All folders</option>
              ${foldersCache.map(folder => `<option value="${folder.id}">${escapeHtml(folder.name)}</option>`).join('')}
            </select>
          </div>
          <textarea class="chat-textarea" id="chatTextarea" placeholder="Summarize my meetings this week" rows="1"></textarea>
        </div>
        <div class="chat-composer-bottom">
          <div class="chat-composer-meta">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.49"/></svg>
            <span class="chat-model-label">Sonnet 4.6</span>
          </div>
          <button class="chat-send-icon" id="chatSendBtn" type="button" aria-label="Send">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 19V5M5 12l7-7 7 7"/></svg>
          </button>
        </div>
      </div>

      <div id="chatResponse" class="chat-response"></div>

      ${recentMeetings.length > 0 ? `
      <div class="chat-section">
        <h3 class="chat-section-label">Recents</h3>
        <div class="chat-recents">
          ${recentMeetings.slice(0, 5).map(m => `
            <a class="chat-recent-item" href="#" data-goto-meeting="${m.id}">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
              <span class="chat-recent-title">${escapeHtml(m.title || 'Untitled meeting')}</span>
              <span class="chat-recent-ago">${formatRelativeTime(m.started_at)}</span>
            </a>
          `).join('')}
        </div>
      </div>
      ` : ''}

      <div class="chat-section">
        <h3 class="chat-section-label">Recipes</h3>
        <div class="chat-recipes">
          <button class="recipe-chip"><span class="chip-icon">/</span> List recent todos</button>
          <button class="recipe-chip"><span class="chip-icon">/</span> Write weekly recap</button>
          <button class="recipe-chip"><span class="chip-icon">/</span> Summarize meeting</button>
          <button class="recipe-chip" onclick="window.location.hash='recipes'">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
            See all
          </button>
        </div>
      </div>
    </div>
  `;

  // Scope pill toggling
  document.querySelectorAll('.scope-pill[data-scope]').forEach(pill => {
    pill.addEventListener('click', () => {
      document.querySelectorAll('.scope-pill[data-scope]').forEach(p => p.classList.remove('active'));
      pill.classList.add('active');
    });
  });

  // Send button
  const sendBtn = document.getElementById('chatSendBtn');
  const textarea = document.getElementById('chatTextarea');

  async function sendChat() {
    const text = textarea.value.trim();
    if (!text) return;
    const scope = document.querySelector('.scope-pill.active')?.dataset.scope || 'all';
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

  // Recent meeting links
  document.querySelectorAll('[data-goto-meeting]').forEach(el => {
    el.addEventListener('click', (e) => {
      e.preventDefault();
      meetingId = el.dataset.gotoMeeting;
      navigate('meeting');
    });
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

}

async function renderPeople() {
  content.innerHTML = `
    <div class="view">
      <div style="display:flex; align-items:center; justify-content:space-between; margin-bottom:20px;">
        <h1 style="font-size:28px; font-weight:700;">People</h1>
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

  const sortedPeople = [...people].sort((a, b) => (b.last_meeting_at || 0) - (a.last_meeting_at || 0));
  const rowsHtml = sortedPeople.map((person, index) => {
    const email = person.email || person.org_name || 'No email';
    const lastNote = formatCompanyLastNote(person.last_meeting_at);
    return `
      <div class="company-row people-row" data-person-id="${person.id}">
        <div class="company-main-cell">
          <div class="person-avatar people-avatar tone-${(index % 4) + 1}">${renderPersonAvatar(person)}</div>
          <div class="company-copy">
            <div class="company-name">${escapeHtml(person.name)}</div>
            <div class="company-domain">${escapeHtml(email)}</div>
          </div>
        </div>
        <div class="company-last-note">${escapeHtml(lastNote)}</div>
        <div class="company-notes-count">${person.meeting_count}</div>
      </div>
    `;
  }).join('');

  const tableHtml = sortedPeople.length > 0
    ? `
      <div class="companies-table-shell">
        <div class="companies-table-head">
          <div>Person</div>
          <div>Last note</div>
          <div>Notes</div>
        </div>
        <div class="companies-table-body">
          ${rowsHtml}
        </div>
      </div>
    `
    : '';

  content.innerHTML = `
    <div class="view companies-view">
      <div class="companies-page-header">
        <h1 class="companies-title">People</h1>
        <div class="companies-header-actions">
          <button class="companies-icon-search" type="button" aria-label="Search people">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
          </button>
          <div class="companies-filter-bar">
            <button class="companies-filter-pill active" type="button">Everyone</button>
            <button class="companies-filter-pill" type="button">People I met</button>
          </div>
        </div>
      </div>
      ${tableHtml}
      <div class="companies-empty-hint ${sortedPeople.length ? '' : 'visible'}">
        <div class="companies-empty-icon">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/></svg>
        </div>
        <p>People from your meetings in Audire will appear here</p>
      </div>

      <div class="inline-form companies-add-form" id="addPersonForm">
        <input type="text" id="personName" placeholder="Name" style="flex:1;" />
        <input type="text" id="personEmail" placeholder="Email (optional)" style="flex:1;" />
        <button class="btn-primary-sm" id="addPersonBtn">Add person</button>
      </div>
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
      <div style="display:flex; align-items:center; justify-content:space-between; margin-bottom:20px;">
        <h1 style="font-size:28px; font-weight:700;">Companies</h1>
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

  const sortedOrgs = [...orgs].sort((a, b) => (b.last_meeting_at || 0) - (a.last_meeting_at || 0));
  const rowsHtml = sortedOrgs.map((org, index) => {
    const displayName = org.domain || org.name;
    const sublabel = org.domain || displayName;
    const lastNote = formatCompanyLastNote(org.last_meeting_at);
    const noteCount = Math.max(1, org.people_count || 0);
    return `
      <div class="company-row" data-company-id="${org.id}">
        <div class="company-main-cell">
          <div class="company-logo tone-${(index % 4) + 1}">${renderCompanyLogo(displayName)}</div>
          <div class="company-copy">
            <div class="company-name">${escapeHtml(displayName)}</div>
            <div class="company-domain">${escapeHtml(sublabel)}</div>
          </div>
        </div>
        <div class="company-last-note">${escapeHtml(lastNote)}</div>
        <div class="company-notes-count">${noteCount}</div>
      </div>
    `;
  }).join('');

  const tableHtml = sortedOrgs.length > 0
    ? `
      <div class="companies-table-shell">
        <div class="companies-table-head">
          <div>Company</div>
          <div>Last note</div>
          <div>Notes</div>
        </div>
        <div class="companies-table-body">
          ${rowsHtml}
        </div>
      </div>
    `
    : '';

  content.innerHTML = `
    <div class="view companies-view">
      <div class="companies-page-header">
        <h1 class="companies-title">Companies</h1>
        <div class="companies-header-actions">
          <button class="companies-icon-search" type="button" aria-label="Search companies">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>
          </button>
          <div class="companies-filter-bar">
            <button class="companies-filter-pill active" type="button">All companies</button>
            <button class="companies-filter-pill" type="button">Companies I met</button>
          </div>
        </div>
      </div>
      ${tableHtml}
      <div class="companies-empty-hint ${sortedOrgs.length ? '' : 'visible'}">
        <div class="companies-empty-icon">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 21h18"/><path d="M5 21V7l8-4v18"/><path d="M19 21V11l-6-4"/><path d="M9 9v.01"/><path d="M9 12v.01"/><path d="M9 15v.01"/><path d="M9 18v.01"/></svg>
        </div>
        <p>Companies of people you meet in Audire will appear here</p>
      </div>
      <div class="inline-form companies-add-form" id="addOrgForm">
        <input type="text" id="orgName" placeholder="Company name" style="flex:1;" />
        <input type="text" id="orgDomain" placeholder="Domain (optional)" style="flex:1;" />
        <button class="btn-primary-sm" id="addOrgBtn">Add company</button>
      </div>
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

function renderCompanyLogo(label) {
  const clean = (label || '').replace(/^www\./i, '');
  const parts = clean.split(/[.\s-]+/).filter(Boolean);
  const initials = (parts[0]?.[0] || clean[0] || '?') + (parts[1]?.[0] || '');
  return escapeHtml(initials.toUpperCase().slice(0, 2));
}

function renderPersonAvatar(person) {
  const name = person?.name || '';
  const parts = name.split(/\s+/).filter(Boolean);
  const initials = ((parts[0]?.[0] || name[0] || '?') + (parts[1]?.[0] || '')).toUpperCase();
  return escapeHtml(initials.slice(0, 2));
}

function formatCompanyLastNote(tsSeconds) {
  if (!tsSeconds) return 'No notes';
  const date = new Date(tsSeconds * 1000);
  const now = new Date();
  const deltaDays = Math.floor((now.setHours(0, 0, 0, 0) - new Date(date).setHours(0, 0, 0, 0)) / 86400000);
  if (deltaDays === 0) return 'Today';
  if (deltaDays === 1) return 'Yesterday';
  if (deltaDays > 1 && deltaDays < 7) {
    return date.toLocaleDateString('en-US', { weekday: 'long' });
  }
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
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

  const isFreshNote = !(note.text || '').trim() && (!note.title || note.title === 'Untitled');

  content.innerHTML = `
    <div class="quick-note-shell">
      <div class="quick-note-topbar">
        <button class="quick-note-back" id="noteBackBtn" aria-label="Back">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m15 18-6-6 6-6"/></svg>
        </button>
        <button class="btn-danger-sm" id="noteDeleteBtn">Delete</button>
      </div>

      <div class="quick-note-header">
        <input class="note-title-input quick-note-title" id="noteTitleInput" value="${escapeHtml(note.title === 'Untitled' ? 'New note' : note.title)}" placeholder="New note" />
        <div class="quick-note-meta-row">
          <span class="quick-note-chip">Today</span>
          <span class="quick-note-chip">Me</span>
          <div class="select-wrap">
            <select id="noteFolderSelect" class="quick-note-folder-select">
              <option value="">Add to folder</option>
              ${foldersCache.map(folder => `
                <option value="${folder.id}" ${note.folder_id === folder.id ? 'selected' : ''}>
                  ${escapeHtml(folder.name)}
                </option>
              `).join('')}
            </select>
          </div>
        </div>
      </div>

      <div class="notes-editor quick-note-editor" contenteditable="true" id="noteBodyEditor">${escapeHtml(note.text)}</div>

      ${isFreshNote ? `
        <div class="quick-note-helper-card">
          <div class="quick-note-helper-icon">+</div>
          <h3>Quick note</h3>
          <p>Use quick notes for thoughts, meeting prep, or action items that are not tied to a scheduled event yet.</p>
          <p>Audire can turn this into a richer note once you start capturing or adding context.</p>
        </div>
      ` : ''}
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

function renderRecorderPill() {
  content.innerHTML = `
    <div class="recorder-pill-view">
      <button class="recorder-pill-logo" id="recorderPillRestoreBtn" type="button" aria-label="Open Audire">
        <span>A</span>
      </button>
      <div class="recorder-pill-center">
        <div class="recorder-pill-dot"></div>
        <div class="recorder-pill-timer" id="recorderPillTimer">00:00</div>
        <div class="recorder-pill-status" id="recorderPillStatus">Listening</div>
        <div class="recorder-pill-preview" id="recorderPillPreview">Waiting for speech...</div>
      </div>
      <div class="recorder-pill-wave" aria-hidden="true">
        <span></span>
        <span></span>
        <span></span>
        <span></span>
        <span></span>
      </div>
      <button class="recorder-pill-stop" id="recorderPillStopBtn" type="button" aria-label="Stop recording">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <rect x="6" y="6" width="12" height="12" rx="2"></rect>
        </svg>
      </button>
    </div>
  `;

  syncRecorderPillUi();

  document.getElementById('recorderPillStopBtn')?.addEventListener('click', () => {
    stopCapture();
  });

  document.getElementById('recorderPillRestoreBtn')?.addEventListener('click', async () => {
    const mainWindow = await WebviewWindow.getByLabel('main');
    await mainWindow?.show().catch(() => {});
    await mainWindow?.unminimize().catch(() => {});
    await mainWindow?.setFocus().catch(() => {});
    await appWindow.hide().catch(() => {});
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
        kind: 'formatted-final',
      }))
    : finals.map(f => ({
        id: null,
        ts_ms: f.ts_ms,
        text: f.text,
        kind: f.formatted ? 'formatted-final' : 'plain-final',
      }));

  if (!transcriptRows.length && !partialText) {
    html = `<p class="transcript-waiting">${isCapturing ? 'Listening...' : 'No transcript yet.'}</p>`;
  }

  for (const row of transcriptRows) {
    html += `
      <div class="transcript-line ${row.kind}" ${row.id ? `data-transcript-segment="${row.id}"` : ''}>
        <span class="transcript-line-time">${escapeHtml(formatDateTime(row.ts_ms))}</span>
        ${escapeHtml(row.text)}
      </div>`;
  }
  if (partialTranscript?.text) {
    html += `<div class="transcript-line partial">${escapeHtml(partialTranscript.text)}</div>`;
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

function formatRelativeTime(tsMs) {
  if (!tsMs) return '';
  const diff = Date.now() - tsMs;
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d`;
  return `${Math.floor(days / 30)}mo`;
}

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

// ---- Init ----
hydrateCaptureUiState();
hydrateSidebarState();
window.addEventListener('storage', (event) => {
  if (event.key !== CAPTURE_UI_STATE_KEY) return;
  hydrateCaptureUiState();
  updateCapturePillText();
  updateCaptureTimer();
  updateTranscript();
  syncRecorderPillUi();
});
startRecorderPillMonitor();
void loadSidebarFolders();
const initHash = location.hash.slice(1) || 'home';
navigate(initHash, false);
