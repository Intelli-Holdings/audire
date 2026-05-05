// Notes view — three-pane workspace (folders | list | editor)
// with date-grouped list, formatting toolbar, and live save.

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';
import { setupAutosizeTextarea } from '../interaction.js';

let appState = null;
let onNavigateToTranscript = null;

let standaloneNotes = [];
let meetingNotes = [];
let folders = [];
let currentScope = { kind: 'all' };
let currentNoteId = null;
let currentNoteType = null;
let saveTimer = null;
let searchQuery = '';

const ICON = {
  search:    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>`,
  newNote:   `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M14 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-8"/><path d="M18.5 2.5a2.121 2.121 0 1 1 3 3L13 14l-4 1 1-4z"/></svg>`,
  edit:      `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z"/></svg>`,
  folder:    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>`,
  notes:     `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>`,
  meeting:   `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><path d="M12 19v3"/></svg>`,
  heading:   `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M6 4v16M18 4v16M6 12h12"/></svg>`,
  checklist: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="m3 7 3 3 5-6"/><path d="m3 17 3 3 5-6"/><path d="M14 7h7"/><path d="M14 17h7"/></svg>`,
  bullet:    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="5" cy="6" r="1.4"/><circle cx="5" cy="12" r="1.4"/><circle cx="5" cy="18" r="1.4"/><path d="M10 6h11M10 12h11M10 18h11"/></svg>`,
  numbered:  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M10 6h11M10 12h11M10 18h11"/><path d="M4 4h2v4M3 13h3l-3 3h3"/></svg>`,
  duplicate: `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><rect width="13" height="13" x="8" y="8" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h2"/></svg>`,
  share:     `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 2v14"/><path d="m6 8 6-6 6 6"/><path d="M5 14v6a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-6"/></svg>`,
  trash:     `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/></svg>`,
  plus:      `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>`,
  pin:       `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/></svg>`,
};

function escapeHtml(s) {
  const d = document.createElement('div');
  d.textContent = s ?? '';
  return d.innerHTML;
}

function startOfDay(d) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

function dateLabelForList(tsSeconds) {
  if (!tsSeconds) return '';
  const date = new Date(tsSeconds * 1000);
  const now = new Date();
  const todayStart = startOfDay(now);
  const dayStart = startOfDay(date);
  const diffDays = Math.floor((todayStart - dayStart) / 86400000);
  if (diffDays === 0) return date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  if (diffDays === 1) return 'Yesterday';
  if (diffDays < 7) return date.toLocaleDateString(undefined, { weekday: 'long' });
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

function dateLabelForEditor(tsSeconds) {
  if (!tsSeconds) return '';
  return new Date(tsSeconds * 1000).toLocaleString(undefined, {
    year: 'numeric', month: 'long', day: 'numeric',
    hour: 'numeric', minute: '2-digit',
  });
}

function groupKeyFor(tsSeconds) {
  if (!tsSeconds) return { key: 'older', label: 'Older', order: 999 };
  const date = new Date(tsSeconds * 1000);
  const now = new Date();
  const todayStart = startOfDay(now);
  const dayStart = startOfDay(date);
  const diffDays = Math.floor((todayStart - dayStart) / 86400000);
  if (diffDays <= 0) return { key: 'today', label: 'Today', order: 0 };
  if (diffDays === 1) return { key: 'yesterday', label: 'Yesterday', order: 1 };
  if (diffDays < 7) return { key: 'previous-7', label: 'Previous 7 Days', order: 2 };
  if (diffDays < 30) return { key: 'previous-30', label: 'Previous 30 Days', order: 3 };
  if (date.getFullYear() === now.getFullYear()) {
    const m = date.toLocaleDateString(undefined, { month: 'long' });
    return { key: `m-${date.getFullYear()}-${date.getMonth()}`, label: m, order: 100 - date.getMonth() };
  }
  const my = date.toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  return { key: `m-${date.getFullYear()}-${date.getMonth()}`, label: my, order: 200 + (now.getFullYear() - date.getFullYear()) * 12 - date.getMonth() };
}

export function initNotesView(state, callbacks = {}) {
  appState = state;
  onNavigateToTranscript = callbacks.onNavigateToTranscript || null;
}

export async function renderNotesView() {
  if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }

  const container = document.getElementById('view-notes');
  if (!container) return;

  await loadData();

  if (appState.selectedFolderId) {
    currentScope = { kind: 'folder', id: appState.selectedFolderId };
  } else if (currentScope.kind === 'folder') {
    currentScope = { kind: 'all' };
  }

  container.innerHTML = `
    <div class="notes-app">
      <aside class="notes-app__folders" aria-label="Folders">
        <div class="notes-app__folders-header">Library</div>
        <div class="notes-app__folder-list" id="notes-folder-list"></div>
      </aside>
      <section class="notes-app__list" aria-label="Notes list">
        <div class="notes-app__list-toolbar">
          <div class="notes-app__search">
            <span class="notes-app__search-icon">${ICON.search}</span>
            <input
              type="search"
              id="notes-search-input"
              class="notes-app__search-input"
              placeholder="Search"
              autocomplete="off"
              spellcheck="false"
            />
          </div>
          <button class="notes-app__icon-btn" id="notes-new-btn" type="button" title="New note" aria-label="New note">${ICON.newNote}</button>
        </div>
        <div class="notes-app__list-header" id="notes-list-header"></div>
        <div class="notes-app__list-scroll" id="notes-list-scroll"></div>
      </section>
      <section class="notes-app__editor" id="notes-editor" aria-label="Note editor"></section>
    </div>
  `;

  renderFolderPane();
  renderNotesList();

  if (currentNoteId && currentNoteType === 'standalone' &&
      standaloneNotes.some(n => n.id === currentNoteId)) {
    await openStandaloneNote(currentNoteId);
  } else if (currentNoteId && currentNoteType === 'meeting' &&
      meetingNotes.some(m => String(m.id) === String(currentNoteId))) {
    await openMeetingNote(String(currentNoteId));
  } else if (appState.selectedStandaloneNoteId) {
    await openStandaloneNote(Number(appState.selectedStandaloneNoteId));
  } else if (appState.selectedMeetingNoteId) {
    await openMeetingNote(String(appState.selectedMeetingNoteId));
  } else {
    renderEditorEmpty();
  }

  document.getElementById('notes-new-btn')?.addEventListener('click', createNewNote);
  document.getElementById('notes-search-input')?.addEventListener('input', (e) => {
    searchQuery = e.target.value.toLowerCase().trim();
    renderNotesList();
  });
}

async function loadData() {
  try {
    const [notesResp, meetingsResp, foldersResp] = await Promise.all([
      invoke('list_standalone_notes'),
      invoke('list_meetings'),
      invoke('list_folders'),
    ]);
    standaloneNotes = notesResp || [];
    meetingNotes = (meetingsResp || []).filter(m => m.note_count > 0 || m.has_structured_notes);
    folders = foldersResp || [];
    appState.foldersCache = folders;
  } catch (e) {
    console.error('Notes data load error:', e);
  }
}

function scopeMatches(scope, item) {
  if (scope.kind === 'all') return true;
  if (scope.kind === 'meetings') return item._kind === 'meeting';
  if (scope.kind === 'folder') return item.folder_id === scope.id;
  return true;
}

function getVisibleItems() {
  const items = [];
  for (const n of standaloneNotes) {
    items.push({
      _kind: 'standalone',
      id: n.id,
      title: n.title || 'New Note',
      preview: (n.text || '').slice(0, 140),
      ts: n.updated_at,
      folder_id: n.folder_id ?? null,
    });
  }
  for (const m of meetingNotes) {
    items.push({
      _kind: 'meeting',
      id: m.id,
      title: m.title || 'Meeting notes',
      preview: m.note_preview || `${m.note_count || 0} note${m.note_count === 1 ? '' : 's'} from this meeting`,
      ts: m.started_at,
      folder_id: m.folder_id ?? null,
    });
  }
  let filtered = items.filter(i => scopeMatches(currentScope, i));
  if (searchQuery) {
    filtered = filtered.filter(i =>
      i.title.toLowerCase().includes(searchQuery) ||
      i.preview.toLowerCase().includes(searchQuery)
    );
  }
  filtered.sort((a, b) => (b.ts || 0) - (a.ts || 0));
  return filtered;
}

function renderFolderPane() {
  const el = document.getElementById('notes-folder-list');
  if (!el) return;

  const allCount = standaloneNotes.length + meetingNotes.length;
  const meetingCount = meetingNotes.length;

  const builtin = [
    { scope: { kind: 'all' }, label: 'All Notes', count: allCount, icon: ICON.notes },
    { scope: { kind: 'meetings' }, label: 'Meetings', count: meetingCount, icon: ICON.meeting },
  ];

  const isActive = (scope) => {
    if (currentScope.kind !== scope.kind) return false;
    if (scope.kind === 'folder') return currentScope.id === scope.id;
    return true;
  };

  let html = '';
  for (const b of builtin) {
    html += `
      <button class="notes-app__folder-row${isActive(b.scope) ? ' is-active' : ''}"
              data-scope-kind="${b.scope.kind}" type="button">
        <span class="notes-app__folder-icon">${b.icon}</span>
        <span class="notes-app__folder-name">${escapeHtml(b.label)}</span>
        <span class="notes-app__folder-count">${b.count}</span>
      </button>
    `;
  }

  if (folders.length) {
    html += `<div class="notes-app__folders-subhead">Folders</div>`;
    for (const f of folders) {
      const scope = { kind: 'folder', id: f.id };
      const count = (f.note_count || 0) + (f.meeting_count || 0);
      html += `
        <button class="notes-app__folder-row${isActive(scope) ? ' is-active' : ''}"
                data-scope-kind="folder" data-folder-id="${f.id}" type="button">
          <span class="notes-app__folder-icon">${ICON.folder}</span>
          <span class="notes-app__folder-name">${escapeHtml(f.name)}</span>
          <span class="notes-app__folder-count">${count}</span>
        </button>
      `;
    }
  }

  html += `
    <button class="notes-app__folder-add" id="notes-add-folder-btn" type="button">
      <span class="notes-app__folder-icon">${ICON.plus}</span>
      <span>New folder</span>
    </button>
  `;

  el.innerHTML = html;

  el.querySelectorAll('.notes-app__folder-row').forEach((row) => {
    row.addEventListener('click', () => {
      const kind = row.dataset.scopeKind;
      if (kind === 'folder') {
        const id = parseInt(row.dataset.folderId, 10);
        currentScope = { kind: 'folder', id };
        appState.selectedFolderId = id;
      } else {
        currentScope = { kind };
        appState.selectedFolderId = null;
      }
      renderFolderPane();
      renderNotesList();
    });
  });

  document.getElementById('notes-add-folder-btn')?.addEventListener('click', () => {
    document.getElementById('add-folder-btn')?.click();
  });
}

function renderNotesList() {
  const headerEl = document.getElementById('notes-list-header');
  const scrollEl = document.getElementById('notes-list-scroll');
  if (!headerEl || !scrollEl) return;

  const items = getVisibleItems();
  const scopeLabel =
    currentScope.kind === 'all' ? 'All Notes' :
    currentScope.kind === 'meetings' ? 'Meetings' :
    folders.find(f => f.id === currentScope.id)?.name || 'Folder';

  headerEl.innerHTML = `
    <h2 class="notes-app__list-title">${escapeHtml(scopeLabel)}</h2>
    <span class="notes-app__list-count">${items.length} ${items.length === 1 ? 'Note' : 'Notes'}</span>
  `;

  if (!items.length) {
    scrollEl.innerHTML = `
      <div class="notes-app__list-empty">
        ${searchQuery ? 'No notes match your search.' : 'No notes yet.'}
      </div>
    `;
    return;
  }

  const grouped = new Map();
  for (const item of items) {
    const g = groupKeyFor(item.ts);
    if (!grouped.has(g.key)) grouped.set(g.key, { label: g.label, order: g.order, items: [] });
    grouped.get(g.key).items.push(item);
  }
  const groups = [...grouped.values()].sort((a, b) => a.order - b.order);

  let html = '';
  for (const group of groups) {
    html += `<div class="notes-app__list-group-label">${escapeHtml(group.label)}</div>`;
    for (const item of group.items) {
      const isActive = currentNoteType === item._kind && String(currentNoteId) === String(item.id);
      const meetingBadge = item._kind === 'meeting'
        ? `<span class="notes-app__list-badge" title="Meeting notes">${ICON.meeting}</span>`
        : '';
      html += `
        <button class="notes-app__list-card${isActive ? ' is-active' : ''}"
                data-note-kind="${item._kind}" data-note-id="${escapeHtml(String(item.id))}"
                type="button">
          <div class="notes-app__list-card-title">
            ${meetingBadge}
            <span>${escapeHtml(item.title)}</span>
          </div>
          <div class="notes-app__list-card-meta">
            <span class="notes-app__list-card-date">${escapeHtml(dateLabelForList(item.ts))}</span>
            <span class="notes-app__list-card-preview">${escapeHtml(item.preview || 'No additional text')}</span>
          </div>
        </button>
      `;
    }
  }

  scrollEl.innerHTML = html;
  scrollEl.querySelectorAll('.notes-app__list-card').forEach((card) => {
    card.addEventListener('click', async () => {
      const kind = card.dataset.noteKind;
      const id = card.dataset.noteId;
      if (kind === 'standalone') {
        await openStandaloneNote(parseInt(id, 10));
      } else {
        await openMeetingNote(String(id));
      }
    });
  });
}

function renderEditorEmpty() {
  const editor = document.getElementById('notes-editor');
  if (!editor) return;
  editor.innerHTML = `
    <div class="notes-app__editor-empty">
      <div class="notes-app__editor-empty-icon">${ICON.notes}</div>
      <p class="notes-app__editor-empty-title">No note selected</p>
      <p class="notes-app__editor-empty-body">Pick a note from the list, or create a new one.</p>
      <button class="notes-app__editor-empty-cta" id="notes-empty-new" type="button">
        ${ICON.newNote} <span>New note</span>
      </button>
    </div>
  `;
  document.getElementById('notes-empty-new')?.addEventListener('click', createNewNote);
}

async function openStandaloneNote(noteId) {
  if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }

  let note;
  try {
    note = await invoke('get_standalone_note', { noteId });
  } catch (e) {
    showToast('Failed to load note', 'error');
    return;
  }

  currentNoteId = noteId;
  currentNoteType = 'standalone';
  appState.selectedStandaloneNoteId = noteId;
  appState.selectedMeetingNoteId = null;

  // refresh active state in list
  document.querySelectorAll('.notes-app__list-card').forEach((c) => {
    const match = c.dataset.noteKind === 'standalone' &&
      String(c.dataset.noteId) === String(noteId);
    c.classList.toggle('is-active', match);
  });

  const editor = document.getElementById('notes-editor');
  if (!editor) return;

  const folderOptions = folders.map(f =>
    `<option value="${f.id}" ${note.folder_id === f.id ? 'selected' : ''}>${escapeHtml(f.name)}</option>`
  ).join('');

  editor.innerHTML = `
    <div class="notes-app__editor-toolbar">
      <div class="notes-app__editor-tools" role="toolbar" aria-label="Note mode">
        <button class="notes-app__tool-btn" id="notes-edit-btn" type="button" title="Edit note" aria-label="Edit note">${ICON.edit}</button>
      </div>
      <div class="notes-app__editor-tools notes-app__editor-tools--center" role="toolbar" aria-label="Formatting">
        <button class="notes-app__tool-btn" data-fmt="heading" type="button" title="Heading" aria-label="Cycle heading style">${ICON.heading}</button>
        <button class="notes-app__tool-btn" data-fmt="checklist" type="button" title="Checklist" aria-label="Toggle checklist">${ICON.checklist}</button>
        <button class="notes-app__tool-btn" data-fmt="bullet" type="button" title="Bullet list" aria-label="Toggle bullet list">${ICON.bullet}</button>
        <button class="notes-app__tool-btn" data-fmt="numbered" type="button" title="Numbered list" aria-label="Toggle numbered list">${ICON.numbered}</button>
      </div>
      <div class="notes-app__editor-tools notes-app__editor-tools--end" role="toolbar" aria-label="Note actions">
        <select class="notes-app__editor-folder" id="notes-folder-select" aria-label="Folder">
          <option value="">No folder</option>
          ${folderOptions}
        </select>
        <button class="notes-app__tool-btn" id="notes-duplicate-btn" type="button" title="Duplicate note" aria-label="Duplicate note">${ICON.duplicate}</button>
        <button class="notes-app__tool-btn" id="notes-export-btn" type="button" title="Export as Markdown" aria-label="Export as Markdown">${ICON.share}</button>
        <button class="notes-app__tool-btn notes-app__tool-btn--danger" id="notes-delete-btn" type="button" title="Delete" aria-label="Delete note">${ICON.trash}</button>
      </div>
    </div>
    <div class="notes-app__editor-scroll">
      <div class="notes-app__editor-content">
        <div class="notes-app__editor-date">${escapeHtml(dateLabelForEditor(note.updated_at))}</div>
        <input
          class="notes-app__editor-title-input"
          id="notes-title-input"
          value="${escapeHtml(note.title === 'New Note' || note.title === 'Untitled' ? '' : note.title)}"
          placeholder="Title"
          spellcheck="true"
        />
        <textarea
          class="notes-app__editor-body"
          id="notes-body-input"
          placeholder="Start writing..."
          spellcheck="true"
        >${escapeHtml(note.text || '')}</textarea>
        <div class="notes-app__editor-status">
          <span id="notes-save-status" data-state="idle">Saved</span>
          <button class="notes-app__editor-retry" id="notes-save-retry" type="button" hidden>Retry</button>
        </div>
      </div>
    </div>
  `;

  const titleInput = document.getElementById('notes-title-input');
  const bodyInput  = document.getElementById('notes-body-input');
  const saveStatus = document.getElementById('notes-save-status');
  const retryBtn   = document.getElementById('notes-save-retry');
  setupAutosizeTextarea(bodyInput, { minRows: 16, maxVh: 0.8 });

  if (!note.title || note.title === 'New Note' || note.title === 'Untitled') {
    titleInput?.focus();
  } else {
    bodyInput?.focus();
    bodyInput?.setSelectionRange(bodyInput.value.length, bodyInput.value.length);
  }

  let lastTitle = titleInput.value;
  let lastBody  = bodyInput.value;

  function setStatus(label, state = 'idle') {
    if (saveStatus) {
      saveStatus.textContent = label;
      saveStatus.dataset.state = state;
    }
    if (retryBtn) retryBtn.hidden = state !== 'error';
  }

  async function save() {
    const title = titleInput.value.trim() || 'New Note';
    const text = bodyInput.value;
    if (title === lastTitle && text === lastBody) {
      setStatus('Saved', 'saved');
      return;
    }
    setStatus('Saving...', 'saving');
    try {
      await invoke('update_standalone_note', { noteId, title, text });
      lastTitle = title;
      lastBody = text;
      // update local cache so the list reflects the change without a round-trip
      const local = standaloneNotes.find(n => n.id === noteId);
      if (local) {
        local.title = title;
        local.text = text;
        local.updated_at = Math.floor(Date.now() / 1000);
      }
      setStatus('Saved', 'saved');
      renderNotesList();
    } catch (e) {
      console.error('Save note error:', e);
      setStatus('Save failed', 'error');
    }
  }

  function schedule() {
    if (saveTimer) clearTimeout(saveTimer);
    setStatus('Editing', 'dirty');
    saveTimer = setTimeout(() => { saveTimer = null; save(); }, 700);
  }

  async function flush() {
    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
    await save();
  }

  titleInput.addEventListener('input', schedule);
  bodyInput.addEventListener('input', schedule);
  titleInput.addEventListener('blur', flush);
  bodyInput.addEventListener('blur', flush);
  bodyInput.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      flush();
    }
  });
  retryBtn?.addEventListener('click', flush);
  document.getElementById('notes-edit-btn')?.addEventListener('click', () => bodyInput?.focus());

  document.getElementById('notes-folder-select')?.addEventListener('change', async (e) => {
    const folderId = e.target.value ? parseInt(e.target.value, 10) : null;
    try {
      await invoke('assign_standalone_note_folder', { noteId, folderId });
      const local = standaloneNotes.find(n => n.id === noteId);
      if (local) local.folder_id = folderId;
      setStatus('Saved', 'saved');
      renderFolderPane();
      renderNotesList();
    } catch {
      showToast('Failed to update folder', 'error');
    }
  });

  document.getElementById('notes-duplicate-btn')?.addEventListener('click', async () => {
    await flush();
    try {
      const copy = await invoke('create_standalone_note', { title: titleInput.value.trim() || 'New Note' });
      await invoke('update_standalone_note', {
        noteId: copy.id,
        title: titleInput.value.trim() || 'New Note',
        text: bodyInput.value,
      });
      showToast('Note duplicated', 'success');
      currentNoteId = copy.id;
      appState.selectedStandaloneNoteId = copy.id;
      appState.selectedMeetingNoteId = null;
      await renderNotesView();
    } catch (e) {
      showToast('Failed to duplicate: ' + e, 'error');
    }
  });

  document.getElementById('notes-export-btn')?.addEventListener('click', () => {
    const title = titleInput.value.trim() || 'New Note';
    const body = bodyInput.value;
    const blob = new Blob([`# ${title}\n\n${body}\n`], { type: 'text/markdown;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${title.replace(/[\\/:*?"<>|]+/g, '-')}.md`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  });

  document.getElementById('notes-delete-btn')?.addEventListener('click', async () => {
    if (!confirm('Delete this note? This cannot be undone.')) return;
    try {
      await invoke('delete_standalone_note', { noteId });
      currentNoteId = null;
      currentNoteType = null;
      if (appState.selectedStandaloneNoteId === noteId) appState.selectedStandaloneNoteId = null;
      appState.selectedMeetingNoteId = null;
      showToast('Note deleted', 'success');
      await renderNotesView();
    } catch (e) {
      showToast('Failed to delete: ' + e, 'error');
    }
  });

  // Formatting toolbar
  editor.querySelectorAll('[data-fmt]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const fmt = btn.dataset.fmt;
      if (fmt === 'heading') cycleHeading(bodyInput);
      else if (fmt === 'checklist') toggleLinePrefix(bodyInput, '- [ ] ', /^- \[[x ]\] /i);
      else if (fmt === 'bullet') toggleLinePrefix(bodyInput, '- ', /^- (?!\[)/);
      else if (fmt === 'numbered') toggleNumbered(bodyInput);
      bodyInput.dispatchEvent(new Event('input', { bubbles: true }));
    });
  });
}

function structuredNoteToMarkdown(note) {
  if (!note) return '';
  const lines = [];
  if (note.summary?.trim()) {
    lines.push('## Summary');
    lines.push(note.summary.trim());
    lines.push('');
  }
  for (const section of note.sections || []) {
    lines.push(`## ${section.label || section.kind || 'Notes'}`);
    if (section.items?.length) {
      for (const item of section.items) {
        if (item.text?.trim()) lines.push(`- ${item.text.trim()}`);
      }
    }
    lines.push('');
  }
  return lines.join('\n').trim();
}

function transcriptToMarkdown(segments = []) {
  if (!segments.length) return '';
  return segments
    .slice()
    .sort((a, b) => a.ts_ms - b.ts_ms)
    .map(seg => {
      const source = seg.source === 'MIC' ? 'You' : 'Speaker';
      return `[${formatShortTime(seg.ts_ms)}] ${source}: ${seg.text}`;
    })
    .join('\n');
}

function getMeetingEditorText(detail) {
  const userText = (detail.user_notes || []).map(n => n.text).join('\n\n').trim();
  if (userText) return userText;

  const structuredText = structuredNoteToMarkdown(detail.structured_note);
  if (structuredText) return structuredText;

  return transcriptToMarkdown(detail.segments || []);
}

function formatShortTime(tsMs) {
  const d = new Date(tsMs);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

async function openMeetingNote(meetingId) {
  if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }

  let detail;
  try {
    detail = await invoke('get_meeting_detail', { meetingId });
  } catch (e) {
    showToast('Failed to load meeting note', 'error');
    return;
  }

  const meeting = detail.meeting;
  currentNoteId = meetingId;
  currentNoteType = 'meeting';
  appState.selectedMeetingNoteId = meetingId;
  appState.selectedStandaloneNoteId = null;
  if (!appState.isCapturing) appState.meetingId = meetingId;

  document.querySelectorAll('.notes-app__list-card').forEach((c) => {
    const match = c.dataset.noteKind === 'meeting' &&
      String(c.dataset.noteId) === String(meetingId);
    c.classList.toggle('is-active', match);
  });

  const editor = document.getElementById('notes-editor');
  if (!editor) return;

  const folderOptions = folders.map(f =>
    `<option value="${f.id}" ${meeting.folder_id === f.id ? 'selected' : ''}>${escapeHtml(f.name)}</option>`
  ).join('');
  const title = meeting.title || 'Meeting Notes';
  const bodyText = getMeetingEditorText(detail);

  editor.innerHTML = `
    <div class="notes-app__editor-toolbar">
      <div class="notes-app__editor-tools" role="toolbar" aria-label="Meeting note mode">
        <button class="notes-app__tool-btn" id="notes-edit-btn" type="button" title="Edit note" aria-label="Edit note">${ICON.edit}</button>
      </div>
      <div class="notes-app__editor-tools notes-app__editor-tools--center" role="toolbar" aria-label="Formatting">
        <button class="notes-app__tool-btn" data-fmt="heading" type="button" title="Heading" aria-label="Cycle heading style">${ICON.heading}</button>
        <button class="notes-app__tool-btn" data-fmt="checklist" type="button" title="Checklist" aria-label="Toggle checklist">${ICON.checklist}</button>
        <button class="notes-app__tool-btn" data-fmt="bullet" type="button" title="Bullet list" aria-label="Toggle bullet list">${ICON.bullet}</button>
        <button class="notes-app__tool-btn" data-fmt="numbered" type="button" title="Numbered list" aria-label="Toggle numbered list">${ICON.numbered}</button>
      </div>
      <div class="notes-app__editor-tools notes-app__editor-tools--end" role="toolbar" aria-label="Meeting note actions">
        <select class="notes-app__editor-folder" id="meeting-notes-folder-select" aria-label="Folder">
          <option value="">No folder</option>
          ${folderOptions}
        </select>
        <button class="notes-app__tool-btn" id="meeting-open-transcript-btn" type="button" title="Open transcript" aria-label="Open transcript">${ICON.meeting}</button>
        <button class="notes-app__tool-btn" id="meeting-export-btn" type="button" title="Export meeting" aria-label="Export meeting">${ICON.share}</button>
        <button class="notes-app__tool-btn notes-app__tool-btn--danger" id="meeting-delete-btn" type="button" title="Delete" aria-label="Delete meeting note">${ICON.trash}</button>
      </div>
    </div>
    <div class="notes-app__editor-scroll">
      <div class="notes-app__editor-content">
        <div class="notes-app__editor-date">${escapeHtml(dateLabelForEditor(meeting.started_at))}</div>
        <input
          class="notes-app__editor-title-input"
          id="meeting-title-input"
          value="${escapeHtml(title)}"
          placeholder="Meeting title"
          spellcheck="true"
        />
        <textarea
          class="notes-app__editor-body"
          id="meeting-notes-body-input"
          placeholder="Write your notes here..."
          spellcheck="true"
        >${escapeHtml(bodyText)}</textarea>
        <div class="notes-app__editor-status">
          <span id="meeting-save-status" data-state="idle">Saved</span>
          <button class="notes-app__editor-retry" id="meeting-save-retry" type="button" hidden>Retry</button>
        </div>
      </div>
    </div>
  `;

  const titleInput = document.getElementById('meeting-title-input');
  const bodyInput = document.getElementById('meeting-notes-body-input');
  const saveStatus = document.getElementById('meeting-save-status');
  const retryBtn = document.getElementById('meeting-save-retry');
  setupAutosizeTextarea(bodyInput, { minRows: 16, maxVh: 0.8 });

  bodyInput?.focus();
  bodyInput?.setSelectionRange(bodyInput.value.length, bodyInput.value.length);

  let lastTitle = titleInput.value.trim() || 'Meeting Notes';
  let lastBody = bodyInput.value;

  function setStatus(label, state = 'idle') {
    if (saveStatus) {
      saveStatus.textContent = label;
      saveStatus.dataset.state = state;
    }
    if (retryBtn) retryBtn.hidden = state !== 'error';
  }

  async function save() {
    const nextTitle = titleInput.value.trim() || 'Meeting Notes';
    const nextBody = bodyInput.value;
    if (nextTitle === lastTitle && nextBody === lastBody) {
      setStatus('Saved', 'saved');
      return;
    }

    setStatus('Saving...', 'saving');
    try {
      if (nextTitle !== lastTitle) {
        await invoke('update_meeting_title', { meetingId, title: nextTitle });
      }
      if (nextBody !== lastBody) {
        await invoke('replace_meeting_notes', { meetingId, text: nextBody });
      }
      lastTitle = nextTitle;
      lastBody = nextBody;
      const local = meetingNotes.find(m => String(m.id) === String(meetingId));
      if (local) {
        local.title = nextTitle;
        local.note_preview = nextBody.slice(0, 140);
        local.note_count = nextBody.trim() ? 1 : 0;
      }
      setStatus('Saved', 'saved');
      renderNotesList();
    } catch (e) {
      console.error('Save meeting note error:', e);
      setStatus('Save failed', 'error');
    }
  }

  function schedule() {
    if (saveTimer) clearTimeout(saveTimer);
    setStatus('Editing', 'dirty');
    saveTimer = setTimeout(() => { saveTimer = null; save(); }, 700);
  }

  async function flush() {
    if (saveTimer) { clearTimeout(saveTimer); saveTimer = null; }
    await save();
  }

  titleInput.addEventListener('input', schedule);
  bodyInput.addEventListener('input', schedule);
  titleInput.addEventListener('blur', flush);
  bodyInput.addEventListener('blur', flush);
  bodyInput.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      flush();
    }
  });
  retryBtn?.addEventListener('click', flush);
  document.getElementById('notes-edit-btn')?.addEventListener('click', () => bodyInput?.focus());

  document.getElementById('meeting-notes-folder-select')?.addEventListener('change', async (e) => {
    const folderId = e.target.value ? parseInt(e.target.value, 10) : null;
    try {
      await invoke('assign_meeting_folder', { meetingId, folderId });
      const local = meetingNotes.find(m => String(m.id) === String(meetingId));
      if (local) local.folder_id = folderId;
      setStatus('Saved', 'saved');
      renderFolderPane();
      renderNotesList();
    } catch {
      showToast('Failed to update folder', 'error');
    }
  });

  document.getElementById('meeting-open-transcript-btn')?.addEventListener('click', async () => {
    await flush();
    appState.meetingId = meetingId;
    if (onNavigateToTranscript) onNavigateToTranscript();
  });

  document.getElementById('meeting-export-btn')?.addEventListener('click', async () => {
    await flush();
    try {
      const resp = await invoke('export', { meetingId, format: 'md' });
      showToast(resp?.path ? `Exported to ${resp.path}` : 'Meeting exported', 'success');
    } catch (e) {
      showToast('Failed to export meeting', 'error');
    }
  });

  document.getElementById('meeting-delete-btn')?.addEventListener('click', async () => {
    if (appState.isCapturing && String(appState.meetingId) === String(meetingId)) {
      showToast('Stop recording before deleting this meeting note.', 'warning');
      return;
    }
    if (!confirm('Delete this meeting note and its transcript? This cannot be undone.')) return;
    try {
      await invoke('delete_meeting', { meetingId });
      currentNoteId = null;
      currentNoteType = null;
      appState.selectedMeetingNoteId = null;
      if (String(appState.meetingId) === String(meetingId)) appState.meetingId = null;
      showToast('Meeting note deleted', 'success');
      await renderNotesView();
    } catch (e) {
      showToast('Failed to delete meeting note: ' + e, 'error');
    }
  });

  editor.querySelectorAll('[data-fmt]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const fmt = btn.dataset.fmt;
      if (fmt === 'heading') cycleHeading(bodyInput);
      else if (fmt === 'checklist') toggleLinePrefix(bodyInput, '- [ ] ', /^- \[[x ]\] /i);
      else if (fmt === 'bullet') toggleLinePrefix(bodyInput, '- ', /^- (?!\[)/);
      else if (fmt === 'numbered') toggleNumbered(bodyInput);
      bodyInput.dispatchEvent(new Event('input', { bubbles: true }));
    });
  });
}

async function createNewNote() {
  try {
    const note = await invoke('create_standalone_note', { title: 'New Note' });
    if (currentScope.kind === 'folder') {
      try {
        await invoke('assign_standalone_note_folder', { noteId: note.id, folderId: currentScope.id });
      } catch { /* non-critical */ }
    }
    currentNoteId = note.id;
    currentNoteType = 'standalone';
    appState.selectedStandaloneNoteId = note.id;
    appState.selectedMeetingNoteId = null;
    await renderNotesView();
  } catch (e) {
    showToast('Failed to create note: ' + e, 'error');
  }
}

// --- Markdown-style line operations on the textarea ---

function getLineRange(textarea) {
  const text = textarea.value;
  const cursor = textarea.selectionStart;
  const lineStart = text.lastIndexOf('\n', cursor - 1) + 1;
  let lineEnd = text.indexOf('\n', cursor);
  if (lineEnd === -1) lineEnd = text.length;
  return { text, cursor, lineStart, lineEnd, line: text.slice(lineStart, lineEnd) };
}

function applyLine(textarea, lineStart, lineEnd, newLine, caretShift = 0) {
  const before = textarea.value.slice(0, lineStart);
  const after = textarea.value.slice(lineEnd);
  textarea.value = before + newLine + after;
  const newCaret = Math.max(lineStart, textarea.selectionStart + caretShift);
  textarea.setSelectionRange(newCaret, newCaret);
  textarea.focus();
}

function toggleLinePrefix(textarea, prefix, matchRegex) {
  const r = getLineRange(textarea);
  if (matchRegex.test(r.line)) {
    const stripped = r.line.replace(matchRegex, '');
    applyLine(textarea, r.lineStart, r.lineEnd, stripped, -(r.line.length - stripped.length));
  } else {
    applyLine(textarea, r.lineStart, r.lineEnd, prefix + r.line, prefix.length);
  }
}

function toggleNumbered(textarea) {
  const r = getLineRange(textarea);
  const num = /^\d+\.\s/;
  if (num.test(r.line)) {
    const stripped = r.line.replace(num, '');
    applyLine(textarea, r.lineStart, r.lineEnd, stripped, -(r.line.length - stripped.length));
  } else {
    const prefix = '1. ';
    applyLine(textarea, r.lineStart, r.lineEnd, prefix + r.line, prefix.length);
  }
}

function cycleHeading(textarea) {
  const r = getLineRange(textarea);
  let newLine, shift;
  if (/^### /.test(r.line)) { newLine = r.line.replace(/^### /, ''); shift = -4; }
  else if (/^## /.test(r.line)) { newLine = r.line.replace(/^## /, '### '); shift = 1; }
  else if (/^# /.test(r.line)) { newLine = r.line.replace(/^# /, '## '); shift = 1; }
  else { newLine = '# ' + r.line; shift = 2; }
  applyLine(textarea, r.lineStart, r.lineEnd, newLine, shift);
}
