// Notes view — two-column layout with note list and editor

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';

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
    clearInterval(noteAutoSaveHandle);
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

  // If we have a previously selected note, re-select it
  if (currentNoteId && currentNoteType === 'standalone') {
    await openStandaloneNote(currentNoteId);
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
    clearInterval(noteAutoSaveHandle);
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
    </div>
    <textarea
      class="note-body text-user"
      id="note-body-input"
      placeholder="Start writing\u2026"
      spellcheck="true"
    >${escapeHtml(note.text || '')}</textarea>
    <div class="note-editor-actions">
      <button class="btn-ghost btn-sm btn-danger" id="note-delete-btn">Delete note</button>
    </div>
  `;

  const titleInput = document.getElementById('note-title-input');
  const bodyInput = document.getElementById('note-body-input');

  // Save on blur
  async function saveNote() {
    const title = titleInput?.value.trim() || 'Untitled';
    const text = bodyInput?.value || '';
    try {
      await invoke('update_standalone_note', { noteId, title, text });
    } catch (e) {
      console.error('Save note error:', e);
    }
  }

  titleInput?.addEventListener('blur', saveNote);
  bodyInput?.addEventListener('blur', saveNote);

  // Auto-save every 8 seconds
  let lastText = bodyInput?.value || '';
  let lastTitle = titleInput?.value || '';
  noteAutoSaveHandle = setInterval(() => {
    const curText = bodyInput?.value || '';
    const curTitle = titleInput?.value || '';
    if (curText !== lastText || curTitle !== lastTitle) {
      lastText = curText;
      lastTitle = curTitle;
      saveNote();
    }
  }, 8000);

  // Folder assignment
  document.getElementById('note-folder-select')?.addEventListener('change', async (e) => {
    const folderId = e.target.value ? parseInt(e.target.value) : null;
    try {
      await invoke('assign_standalone_note_folder', { noteId, folderId });
    } catch (err) {
      showToast('Failed to update folder', 'error');
    }
  });

  // Delete
  document.getElementById('note-delete-btn')?.addEventListener('click', async () => {
    try {
      await invoke('delete_standalone_note', { noteId });
      currentNoteId = null;
      currentNoteType = null;
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
