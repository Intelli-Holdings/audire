// Notes view — two-column layout with note list and editor

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';
import { setupAutosizeTextarea } from '../interaction.js';

let appState = null;
let onNavigateToTranscript = null;
let currentNoteId = null;
let currentNoteType = null; // 'standalone' | 'meeting'
let noteAutoSaveHandle = null;
let allStandaloneNotes = [];
let allMeetingNotes = [];
let foldersCache = [];

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
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
  if (diffDays < 7) return date.toLocaleDateString('en-US', { weekday: 'long' });
  return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

export function initNotesView(state, callbacks = {}) {
  appState = state;
  onNavigateToTranscript = callbacks.onNavigateToTranscript || null;
}

export async function renderNotesView() {
  if (noteAutoSaveHandle) {
    clearTimeout(noteAutoSaveHandle);
    noteAutoSaveHandle = null;
  }

  const container = document.getElementById('view-notes');
  if (!container) return;

  // Load data
  try {
    [allStandaloneNotes, allMeetingNotes, foldersCache] = await Promise.all([
      invoke('list_standalone_notes'),
      invoke('list_meetings'),
      invoke('list_folders'),
    ]);
    appState.foldersCache = foldersCache;
  } catch (e) {
    console.error('Notes data load error:', e);
  }

  const meetingsWithNotes = allMeetingNotes.filter(m => m.note_count > 0 || m.has_structured_notes);

  container.innerHTML = `
    <div class="notes-view-layout">
      <div class="notes-list-pane">
        <div class="notes-list-toolbar">
          <input type="text" class="search-input" id="note-search" placeholder="Search notes\u2026" />
          <button class="btn-primary btn-sm" id="new-note-btn">+ New</button>
        </div>
        <div class="notes-list-scroll" id="notes-list-scroll">
          ${renderNoteListItems(allStandaloneNotes, meetingsWithNotes)}
        </div>
      </div>
      <div class="note-editor-pane" id="note-editor-pane">
        <div class="note-editor-empty">
          <div class="empty-state">
            <p class="empty-state-title">Select a note</p>
            <p class="empty-state-body">Choose a note from the list or create a new one.</p>
          </div>
        </div>
      </div>
    </div>
  `;

  // Bind events
  document.getElementById('new-note-btn')?.addEventListener('click', createNewNote);

  // Search
  document.getElementById('note-search')?.addEventListener('input', (e) => {
    const query = e.target.value.toLowerCase();
    filterNoteList(query);
  });

  // Note list clicks
  bindNoteListClicks();

  const selectedStandaloneId = appState.selectedStandaloneNoteId || currentNoteId;
  if (selectedStandaloneId && currentNoteType !== 'meeting') {
    await openStandaloneNote(Number(selectedStandaloneId));
  }
}

function renderNoteListItems(standaloneNotes, meetingNotes) {
  let html = '';

  if (standaloneNotes.length > 0) {
    html += '<div class="notes-list-group-label">Notes</div>';
    for (const n of standaloneNotes) {
      const dateStr = formatRelativeDate(n.updated_at);
      const preview = (n.text || '').slice(0, 60);
      html += `
        <button class="note-list-item" data-note-type="standalone" data-note-id="${n.id}">
          <span class="note-item-title truncate">${escapeHtml(n.title || 'Untitled')}</span>
          <span class="note-item-meta text-xs text-muted">${escapeHtml(dateStr)}${preview ? ' \u00B7 ' + escapeHtml(preview) : ''}</span>
        </button>
      `;
    }
  }

  if (meetingNotes.length > 0) {
    html += '<div class="notes-list-group-label">Meeting Notes</div>';
    const sorted = [...meetingNotes].sort((a, b) => (b.started_at || 0) - (a.started_at || 0));
    for (const m of sorted) {
      const title = m.title || 'Meeting notes';
      const dateStr = formatRelativeDate(m.started_at);
      html += `
        <button class="note-list-item" data-note-type="meeting" data-note-id="${escapeHtml(m.id)}">
          <span class="note-item-title truncate">${escapeHtml(title)}</span>
          <span class="note-item-meta text-xs text-muted">${escapeHtml(dateStr)} \u00B7 ${m.note_count} note${m.note_count === 1 ? '' : 's'}</span>
        </button>
      `;
    }
  }

  if (!standaloneNotes.length && !meetingNotes.length) {
    html = `
      <div class="empty-state" style="padding: var(--space-8);">
        <p class="empty-state-title">No notes yet</p>
        <p class="empty-state-body">Click "+ New" to create your first note.</p>
      </div>
    `;
  }

  return html;
}

function bindNoteListClicks() {
  document.querySelectorAll('.note-list-item').forEach(el => {
    el.addEventListener('click', async () => {
      const type = el.dataset.noteType;
      const id = el.dataset.noteId;

      // Update active state
      document.querySelectorAll('.note-list-item').forEach(i => i.classList.remove('active'));
      el.classList.add('active');

      if (type === 'standalone') {
        await openStandaloneNote(parseInt(id));
      } else if (type === 'meeting') {
        openMeetingNote(id);
      }
    });
  });
}

async function createNewNote() {
  try {
    const note = await invoke('create_standalone_note', { title: 'Untitled' });
    currentNoteId = note.id;
    currentNoteType = 'standalone';
    // Re-render to show new note in list
    await renderNotesView();
    await openStandaloneNote(note.id);
  } catch (e) {
    showToast('Failed to create note: ' + e, 'error');
  }
}

async function openStandaloneNote(noteId) {
  currentNoteId = noteId;
  currentNoteType = 'standalone';

  if (noteAutoSaveHandle) {
    clearTimeout(noteAutoSaveHandle);
    noteAutoSaveHandle = null;
  }

  let note;
  try {
    note = await invoke('get_standalone_note', { noteId });
  } catch (e) {
    showToast('Failed to load note', 'error');
    return;
  }

  // Mark active in list
  document.querySelectorAll('.note-list-item').forEach(i => {
    i.classList.toggle('active',
      i.dataset.noteType === 'standalone' && parseInt(i.dataset.noteId) === noteId);
  });

  const editorPane = document.getElementById('note-editor-pane');
  const folderOptions = foldersCache.map(f =>
    `<option value="${f.id}" ${note.folder_id === f.id ? 'selected' : ''}>${escapeHtml(f.name)}</option>`
  ).join('');

  editorPane.innerHTML = `
    <input
      class="note-editor-title"
      id="note-title-input"
      value="${escapeHtml(note.title === 'Untitled' ? '' : note.title)}"
      placeholder="Note title"
    />
    <div class="note-editor-meta">
      <span class="note-editor-meta-item">${escapeHtml(formatRelativeDate(note.updated_at))}</span>
      <select class="note-editor-folder-select" id="note-folder-select">
        <option value="">No folder</option>
        ${folderOptions}
      </select>
      <span class="note-save-status" id="note-save-status" role="status" aria-live="polite">Saved</span>
      <button class="note-save-retry" id="note-save-retry" type="button" hidden>Retry</button>
    </div>
    <textarea
      class="note-body text-user"
      id="note-body-input"
      placeholder="Start writing\u2026"
      spellcheck="true"
    >${escapeHtml(note.text || '')}</textarea>
    <div class="note-editor-actions">
      <button class="btn-ghost btn-sm btn-danger" id="note-delete-btn" type="button">Delete note</button>
    </div>
  `;

  const titleInput = document.getElementById('note-title-input');
  const bodyInput = document.getElementById('note-body-input');
  const saveStatus = document.getElementById('note-save-status');
  const retryBtn = document.getElementById('note-save-retry');
  setupAutosizeTextarea(bodyInput, { minRows: 14, maxVh: 0.62 });
  if (note.title === 'Untitled') {
    titleInput?.focus();
  } else {
    bodyInput?.focus();
  }

  let lastText = bodyInput?.value || '';
  let lastTitle = titleInput?.value || '';

  function setSaveStatus(label, state = 'idle') {
    if (!saveStatus) return;
    saveStatus.textContent = label;
    saveStatus.dataset.state = state;
    if (retryBtn) retryBtn.hidden = state !== 'error';
  }

  async function saveNote() {
    const title = titleInput?.value.trim() || 'Untitled';
    const text = bodyInput?.value || '';
    if (title === lastTitle && text === lastText) {
      setSaveStatus('Saved');
      return;
    }
    setSaveStatus('Saving...', 'saving');
    try {
      await invoke('update_standalone_note', { noteId, title, text });
      lastTitle = title;
      lastText = text;
      setSaveStatus('Saved', 'saved');
    } catch (e) {
      console.error('Save note error:', e);
      setSaveStatus('Save failed', 'error');
    }
  }

  function scheduleSave() {
    if (noteAutoSaveHandle) clearTimeout(noteAutoSaveHandle);
    setSaveStatus('Unsaved', 'dirty');
    noteAutoSaveHandle = setTimeout(() => {
      noteAutoSaveHandle = null;
      saveNote();
    }, 900);
  }

  async function flushSave() {
    if (noteAutoSaveHandle) {
      clearTimeout(noteAutoSaveHandle);
      noteAutoSaveHandle = null;
    }
    await saveNote();
  }

  titleInput?.addEventListener('input', scheduleSave);
  bodyInput?.addEventListener('input', scheduleSave);
  titleInput?.addEventListener('blur', flushSave);
  bodyInput?.addEventListener('blur', flushSave);
  bodyInput?.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      flushSave();
    }
  });
  retryBtn?.addEventListener('click', flushSave);

  // Folder assignment
  document.getElementById('note-folder-select')?.addEventListener('change', async (e) => {
    const folderId = e.target.value ? parseInt(e.target.value) : null;
    try {
      await invoke('assign_standalone_note_folder', { noteId, folderId });
      setSaveStatus('Saved', 'saved');
    } catch (err) {
      showToast('Failed to update folder', 'error');
    }
  });

  // Delete
  document.getElementById('note-delete-btn')?.addEventListener('click', async () => {
    if (!confirm('Delete this note? This cannot be undone.')) return;
    try {
      await invoke('delete_standalone_note', { noteId });
      currentNoteId = null;
      currentNoteType = null;
      if (appState.selectedStandaloneNoteId === noteId) appState.selectedStandaloneNoteId = null;
      showToast('Note deleted', 'success');
      await renderNotesView();
    } catch (e) {
      showToast('Failed to delete note: ' + e, 'error');
    }
  });
}

function openMeetingNote(meetingId) {
  // Navigate to transcript view for this meeting
  appState.meetingId = meetingId;
  if (onNavigateToTranscript) onNavigateToTranscript();
}

function filterNoteList(query) {
  document.querySelectorAll('.note-list-item').forEach(el => {
    const title = el.querySelector('.note-item-title')?.textContent.toLowerCase() || '';
    const meta = el.querySelector('.note-item-meta')?.textContent.toLowerCase() || '';
    const matches = !query || title.includes(query) || meta.includes(query);
    el.style.display = matches ? '' : 'none';
  });
}
