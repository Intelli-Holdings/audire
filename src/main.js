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

  // Global error boundary for unhandled IPC failures
  window.addEventListener('unhandledrejection', e => {
    const msg = e.reason?.toString() || '';
    if (msg.includes('tauri') || msg.includes('invoke')) {
      showToast('IPC error: ' + msg, 'error');
    }
  });
});
