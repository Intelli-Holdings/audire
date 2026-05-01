// Audire — Main entry point
// Imports all modules and wires them together

import './style.css';
import { initSidebar, showView, autoSelectProvider, startCapture } from './sidebar.js';
import { showToast } from './toast.js';
import { initTranscriptView, renderTranscriptView } from './views/transcript.js';
import { initNotesView, renderNotesView } from './views/notes.js';
import { initPeopleView, renderPeopleView } from './views/people.js';
import { initCompaniesView, renderCompaniesView } from './views/companies.js';
import { initSettingsView, renderSettingsView } from './views/settings.js';
import { initHomeView, renderHomeView } from './views/home.js';
import { initMyNotesView, renderMyNotesView } from './views/mynotes.js';
import { initSharedView, renderSharedView } from './views/shared.js';
import { initChatView, renderChatView } from './views/chat.js';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// ---- App State ----
// Single shared state object passed by reference to all modules
const AppState = {
  currentView: 'home',
  meetingId: null,
  isCapturing: false,
  captureStartedAt: null,
  captureStatus: '',
  captureValidated: false,
  captureAudioLevel: 0,
  finals: [],
  partialText: '',
  foldersCache: [],
  selectedProvider: 'assemblyai',
  selectedFolderId: null,
  selectedStandaloneNoteId: null,
};

// Expose toast globally for convenience
window.showToast = showToast;

// ---- View rendering dispatch ----
function onViewChange(viewName) {
  switch (viewName) {
    case 'home':
      renderHomeView();
      break;
    case 'shared':
      renderSharedView();
      break;
    case 'chat':
      renderChatView();
      break;
    case 'mynotes':
      renderMyNotesView();
      break;
    case 'notes':
      renderNotesView();
      break;
    case 'transcript':
      renderTranscriptView();
      break;
    case 'people':
      renderPeopleView();
      break;
    case 'companies':
      renderCompaniesView();
      break;
    case 'settings':
      renderSettingsView();
      break;
  }
}

// ---- View history for back/forward nav ----
const viewHistory = ['home'];
let viewHistoryIndex = 0;

function navigateBack() {
  if (viewHistoryIndex > 0) {
    viewHistoryIndex--;
    const prev = viewHistory[viewHistoryIndex];
    showView(prev);
  }
}

function navigateForward() {
  if (viewHistoryIndex < viewHistory.length - 1) {
    viewHistoryIndex++;
    const next = viewHistory[viewHistoryIndex];
    showView(next);
  }
}

// Wrap showView to track history
const originalShowView = showView;
function trackedShowView(viewName) {
  // Only add to history if it's a new navigation (not back/forward)
  if (viewHistory[viewHistoryIndex] !== viewName) {
    // Trim forward history
    viewHistory.length = viewHistoryIndex + 1;
    viewHistory.push(viewName);
    viewHistoryIndex = viewHistory.length - 1;
  }
}

// ---- Initialize ----
document.addEventListener('DOMContentLoaded', async () => {
  // Init all modules with shared state
  initSidebar(AppState, (viewName) => {
    trackedShowView(viewName);
    onViewChange(viewName);
  });

  initTranscriptView(AppState, {
    onCaptureStop: () => {},
    onNavigateHome: () => showView('home'),
  });
  initNotesView(AppState, {
    onNavigateToTranscript: () => showView('transcript'),
  });
  initHomeView(AppState, {
    onNavigateToTranscript: () => showView('transcript'),
  });
  initMyNotesView(AppState, {
    onNavigateToTranscript: () => showView('transcript'),
  });
  initSharedView(AppState);
  initChatView(AppState);
  initPeopleView(AppState);
  initCompaniesView(AppState);
  initSettingsView(AppState);

  // Settings button in sidebar footer
  document.getElementById('user-card-btn')
    ?.addEventListener('click', () => showView('settings'));

  // Populate user card from stored display name (set in Settings → Preferences).
  // Default to the app name "Audire" when no name is set so we never show a
  // hardcoded "Audire User" placeholder.
  try {
    const storedName = localStorage.getItem('audire.user.displayName') || '';
    const cardName = document.querySelector('.user-card-name');
    if (cardName) cardName.textContent = storedName || 'Audire';
    const cardAvatar = document.querySelector('.user-card .user-avatar');
    if (cardAvatar) cardAvatar.textContent = (storedName.trim()[0] || 'A').toUpperCase();
  } catch { /* localStorage unavailable */ }

  // Sidebar collapse + floating restore button
  const SIDEBAR_COLLAPSED_KEY = 'audire.sidebar.collapsed';
  const sidebarEl = document.getElementById('sidebar');
  const sidebarCollapseBtn = document.getElementById('sidebar-collapse-btn');
  const sidebarRestoreBtn = document.getElementById('sidebarRestoreBtn');

  function setSidebarCollapsed(collapsed) {
    if (!sidebarEl) return;
    sidebarEl.classList.toggle('collapsed', collapsed);
    document.body.classList.toggle('sidebar-hidden', collapsed);
    try {
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, collapsed ? '1' : '0');
    } catch { /* ignore */ }
  }

  sidebarCollapseBtn?.addEventListener('click', () => setSidebarCollapsed(true));
  sidebarRestoreBtn?.addEventListener('click', () => setSidebarCollapsed(false));

  try {
    setSidebarCollapsed(localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === '1');
  } catch {
    setSidebarCollapsed(false);
  }

  // Cross-view navigation requests (e.g. chat → settings, chat → transcript).
  document.addEventListener('audire:navigate', (e) => {
    const view = e?.detail?.view;
    if (typeof view === 'string') showView(view);
  });

  // Titlebar nav buttons
  document.getElementById('nav-back-btn')
    ?.addEventListener('click', navigateBack);
  document.getElementById('nav-forward-btn')
    ?.addEventListener('click', navigateForward);
  document.getElementById('nav-home-btn')
    ?.addEventListener('click', () => showView('home'));

  // Quick note button — create a standalone note and navigate to notes view
  document.getElementById('quick-note-btn')
    ?.addEventListener('click', async () => {
      try {
        const note = await invoke('create_standalone_note', { title: 'Untitled' });
        AppState.selectedStandaloneNoteId = note.id;
        showView('notes');
      } catch (e) {
        showToast('Failed to create note: ' + e, 'error');
      }
    });

  // Window controls (Tauri custom titlebar)
  document.getElementById('win-minimize')?.addEventListener('click', async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      getCurrentWindow().minimize();
    } catch { /* ignore in dev */ }
  });
  document.getElementById('win-maximize')?.addEventListener('click', async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const win = getCurrentWindow();
      if (await win.isMaximized()) {
        win.unmaximize();
      } else {
        win.maximize();
      }
    } catch { /* ignore in dev */ }
  });
  document.getElementById('win-close')?.addEventListener('click', async () => {
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      getCurrentWindow().close();
    } catch { /* ignore in dev */ }
  });

  // Auto-select best available ASR provider
  await autoSelectProvider();

  // Default view
  showView('home');

  // ---- Detection system ----
  // Listen for meeting-about-to-start prompts from the background detector
  listen('detection://prompt', (event) => {
    showDetectionPrompt(event.payload);
  });

  // Start the background detection loop
  invoke('start_detector').catch(e => {
    console.warn('Failed to start detector:', e);
  });

  // Global error boundary for unhandled IPC failures
  window.addEventListener('unhandledrejection', e => {
    const msg = e.reason?.toString() || '';
    if (msg.includes('tauri') || msg.includes('invoke')) {
      showToast('IPC error: ' + msg, 'error');
    }
  });
});

// ---- Detection Prompt UI ----

function showDetectionPrompt(payload) {
  // Remove any existing prompt
  document.querySelector('.detection-prompt')?.remove();

  const { external_id, provider, title, start, attendees } = payload;
  const startDate = new Date(start.length === 10 ? start + 'T00:00:00' : start);
  const timeStr = startDate.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  const el = document.createElement('div');
  el.className = 'detection-prompt';
  el.innerHTML = `
    <div class="detection-prompt-title">Meeting starting soon</div>
    <div class="detection-prompt-event">${escapeHtmlSimple(title)}</div>
    <div class="detection-prompt-time">${escapeHtmlSimple(timeStr)}</div>
    <div class="detection-prompt-actions">
      <button class="btn-primary btn-sm" data-action="accept">Record</button>
      <button class="btn-ghost btn-sm" data-action="dismiss">Dismiss</button>
    </div>
  `;

  el.querySelector('[data-action="accept"]').addEventListener('click', async () => {
    try {
      await invoke('respond_to_detection_prompt', {
        externalId: external_id,
        provider,
        action: 'accept',
        attendees: attendees || null,
      });
      el.remove();
      showToast('Recording started', 'success');
    } catch (e) {
      showToast('Failed to start recording: ' + e, 'error');
    }
  });

  el.querySelector('[data-action="dismiss"]').addEventListener('click', async () => {
    try {
      await invoke('respond_to_detection_prompt', {
        externalId: external_id,
        provider,
        action: 'dismiss',
        attendees: null,
      });
    } catch { /* ignore */ }
    el.remove();
  });

  document.body.appendChild(el);

  // Auto-dismiss after 5 minutes
  setTimeout(() => {
    if (el.parentNode) {
      invoke('respond_to_detection_prompt', {
        externalId: external_id,
        provider,
        action: 'expired',
        attendees: null,
      }).catch(() => {});
      el.remove();
    }
  }, 5 * 60 * 1000);
}

function escapeHtmlSimple(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}
