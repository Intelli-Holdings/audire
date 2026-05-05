// Sidebar component — navigation, folder list, search, folder modal

import { invoke } from '@tauri-apps/api/core';
import { showToast } from './toast.js';
import { setSettingsSection } from './views/settings.js';
import { setTextareaValue, setupAutosizeTextarea, trapDialogFocus } from './interaction.js';

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
  const navViewName = viewName === 'notes' ? 'mynotes' : viewName;
  // Update nav items
  document.querySelectorAll('.nav-item').forEach(i => {
    const active = i.dataset.view === navViewName
      && !(navViewName === 'mynotes' && appState.selectedFolderId && i.dataset.view === 'mynotes');
    i.classList.toggle('active', active);
    if (active) {
      i.setAttribute('aria-current', 'page');
    } else {
      i.removeAttribute('aria-current');
    }
  });
  // Update folder items
  document.querySelectorAll('.sidebar-folder-item').forEach(i => {
    const active = (viewName === 'mynotes' || viewName === 'notes')
      && i.dataset.folderId
      && Number(i.dataset.folderId) === appState.selectedFolderId;
    i.classList.toggle('active', active);
    if (active) {
      i.setAttribute('aria-current', 'page');
    } else {
      i.removeAttribute('aria-current');
    }
  });
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

  // Pre-flight: BYOK check. If no ASR key is configured we surface a clear
  // explanation + jump to Settings → API Keys instead of letting start_capture
  // fail with an opaque MissingKey error.
  let hasAsrKey = false;
  for (const provider of ['assemblyai', 'deepgram']) {
    try {
      if (await invoke('has_api_key', { provider })) {
        hasAsrKey = true;
        break;
      }
    } catch { /* ignore */ }
  }
  if (!hasAsrKey) {
    showToast('Add a Deepgram or AssemblyAI key in Settings → API Keys to start recording.', 'info');
    setSettingsSection('connectors');
    showView('settings');
    return;
  }

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
    item.addEventListener('click', () => {
      if (item.dataset.view === 'mynotes') appState.selectedFolderId = null;
      showView(item.dataset.view);
    });
    item.addEventListener('keydown', e => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        if (item.dataset.view === 'mynotes') appState.selectedFolderId = null;
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
      <button class="sidebar-folder-item" data-view="folder-${f.id}" data-folder-id="${f.id}" type="button" aria-label="Open folder ${escapeHtml(f.name)}">
        <svg class="sidebar-folder-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
        </svg>
        <span class="truncate">${escapeHtml(f.name)}</span>
      </button>
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
  const descInput = document.getElementById('folder-desc-input');
  let restoreFocusTo = null;
  let releaseFocusTrap = null;

  if (!modal || !addBtn) return;
  setupAutosizeTextarea(descInput, { minRows: 3, maxVh: 0.28 });

  const resetFields = () => {
    if (nameInput) nameInput.value = '';
    if (descInput) setTextareaValue(descInput, '');
    if (createBtn) createBtn.disabled = true;
  };

  const openModal = () => {
    restoreFocusTo = document.activeElement;
    modal.style.display = 'flex';
    modal.removeAttribute('aria-hidden');
    releaseFocusTrap = trapDialogFocus(modal, {
      initialFocus: nameInput,
      restoreFocusTo,
      restoreFocus: false,
      onEscape: closeModal,
    });
  };

  const closeModal = () => {
    releaseFocusTrap?.();
    releaseFocusTrap = null;
    modal.style.display = 'none';
    modal.setAttribute('aria-hidden', 'true');
    resetFields();
    if (restoreFocusTo && typeof restoreFocusTo.focus === 'function') {
      restoreFocusTo.focus();
    }
    restoreFocusTo = null;
  };

  addBtn.addEventListener('click', openModal);

  cancelBtn?.addEventListener('click', closeModal);

  // Close on overlay click
  modal.addEventListener('click', (e) => {
    if (e.target === modal) {
      closeModal();
    }
  });

  nameInput?.addEventListener('input', () => {
    if (createBtn) createBtn.disabled = !nameInput.value.trim();
  });

  createBtn?.addEventListener('click', async () => {
    const name = nameInput?.value.trim();
    if (!name) return;
    const desc = descInput?.value.trim() || null;

    try {
      createBtn.disabled = true;
      createBtn.textContent = 'Creating...';
      await invoke('create_folder', { name, kind: 'private', color: null, description: desc });
      showToast('Folder created', 'success');
      closeModal();
      await loadFolders();
    } catch (e) {
      showToast('Failed to create folder: ' + e, 'error');
    } finally {
      createBtn.textContent = 'Create';
      createBtn.disabled = !nameInput?.value.trim();
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
