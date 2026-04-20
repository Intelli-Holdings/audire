// Sidebar component — navigation, folder list, search, folder modal

import { invoke } from '@tauri-apps/api/core';
import { showToast } from './toast.js';

let appState = null;
let onViewChange = null;

export function initSidebar(state, viewChangeCallback) {
  appState = state;
  onViewChange = viewChangeCallback;
  initNavigation();
  initFolderModal();
  loadFolders();
  initSearch();
}

export function showView(viewName) {
  // Update nav items
  document.querySelectorAll('.nav-item').forEach(i =>
    i.classList.toggle('active', i.dataset.view === viewName));
  // Update folder items
  document.querySelectorAll('.sidebar-folder-item').forEach(i =>
    i.classList.toggle('active', i.dataset.view === viewName));
  // Update views
  document.querySelectorAll('.view').forEach(v =>
    v.classList.toggle('active', v.id === `view-${viewName}`));
  appState.currentView = viewName;
  if (onViewChange) onViewChange(viewName);
}

// Auto-select the best available provider (background, no UI)
export async function autoSelectProvider() {
  const providers = ['assemblyai', 'deepgram'];
  for (const provider of providers) {
    try {
      const hasKey = await invoke('has_api_key', { provider });
      if (hasKey) {
        appState.selectedProvider = provider;
        return provider;
      }
    } catch { /* continue */ }
  }
  appState.selectedProvider = 'assemblyai';
  return 'assemblyai';
}

// Start a new capture session (called from home view or titlebar)
// If calendarEvent is provided, attendees are auto-imported into people/companies.
export async function startCapture(calendarEvent = null) {
  if (appState.isCapturing) return;

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

    // Auto-import attendees from calendar event
    if (calendarEvent && calendarEvent.attendees && calendarEvent.attendees.length > 0) {
      try {
        await invoke('import_event_attendees', {
          meetingId: resp.meeting_id,
          attendees: calendarEvent.attendees,
        });
      } catch (e) {
        console.warn('Failed to import calendar attendees:', e);
      }
      // Set meeting title from calendar event
      if (calendarEvent.title) {
        try {
          await invoke('update_meeting_title', {
            meetingId: resp.meeting_id,
            title: calendarEvent.title,
          });
        } catch { /* non-critical */ }
      }
    }

    showToast('Recording started', 'success');
    showView('transcript');
  } catch (err) {
    appState.isCapturing = false;
    showToast('Could not start capture: ' + err, 'error');
  }
}

export async function stopCapture() {
  if (!appState.isCapturing) return;
  appState.isCapturing = false;
  try {
    await invoke('stop_capture', { meetingId: appState.meetingId });
    showToast('Notes ready', 'success');
  } catch (err) {
    showToast('Error stopping capture: ' + err, 'error');
  }
}

function initNavigation() {
  document.querySelectorAll('.nav-item').forEach(item => {
    item.addEventListener('click', () => showView(item.dataset.view));
    item.addEventListener('keydown', e => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        showView(item.dataset.view);
      }
    });
  });

  // Arrow key navigation within sidebar
  const nav = document.querySelector('.sidebar-nav');
  if (nav) {
    nav.addEventListener('keydown', e => {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        const items = Array.from(nav.querySelectorAll('.nav-item'));
        const current = items.findIndex(i => i === document.activeElement);
        const next = e.key === 'ArrowDown'
          ? Math.min(current + 1, items.length - 1)
          : Math.max(current - 1, 0);
        items[next]?.focus();
      }
    });
  }
}

function initSearch() {
  const searchInput = document.getElementById('sidebar-search');
  if (!searchInput) return;

  // Ctrl+K to focus search
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
      e.preventDefault();
      searchInput.focus();
    }
  });
}

async function loadFolders() {
  const listEl = document.getElementById('sidebar-folder-list');
  if (!listEl) return;

  try {
    const folders = await invoke('list_folders');
    appState.foldersCache = folders;

    listEl.innerHTML = folders.map(f => `
      <a class="sidebar-folder-item" data-view="folder-${f.id}" data-folder-id="${f.id}" role="button" tabindex="0">
        <svg class="sidebar-folder-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
        </svg>
        <span class="truncate">${escapeHtml(f.name)}</span>
      </a>
    `).join('');

    // Bind folder clicks — navigate to mynotes with folder filter
    listEl.querySelectorAll('.sidebar-folder-item').forEach(item => {
      item.addEventListener('click', () => {
        appState.selectedFolderId = parseInt(item.dataset.folderId);
        showView('mynotes');
      });
    });
  } catch (e) {
    console.error('Load folders error:', e);
  }
}

function initFolderModal() {
  const modal = document.getElementById('folder-modal');
  const addBtn = document.getElementById('add-folder-btn');
  const cancelBtn = document.getElementById('folder-cancel-btn');
  const createBtn = document.getElementById('folder-create-btn');
  const nameInput = document.getElementById('folder-name-input');

  if (!modal || !addBtn) return;

  addBtn.addEventListener('click', () => {
    modal.style.display = 'flex';
    nameInput?.focus();
  });

  cancelBtn?.addEventListener('click', () => {
    modal.style.display = 'none';
    if (nameInput) nameInput.value = '';
    const descInput = document.getElementById('folder-desc-input');
    if (descInput) descInput.value = '';
  });

  // Close on overlay click
  modal.addEventListener('click', (e) => {
    if (e.target === modal) {
      modal.style.display = 'none';
    }
  });

  createBtn?.addEventListener('click', async () => {
    const name = nameInput?.value.trim();
    if (!name) return;
    const desc = document.getElementById('folder-desc-input')?.value.trim() || null;

    try {
      await invoke('create_folder', { name, kind: 'private', color: null, description: desc });
      showToast('Folder created', 'success');
      modal.style.display = 'none';
      nameInput.value = '';
      const descInput = document.getElementById('folder-desc-input');
      if (descInput) descInput.value = '';
      await loadFolders();
    } catch (e) {
      showToast('Failed to create folder: ' + e, 'error');
    }
  });

  // Enter key in name input creates folder
  nameInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      createBtn?.click();
    }
  });
}

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}
