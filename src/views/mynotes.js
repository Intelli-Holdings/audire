// My Notes view — folder-scoped private notes view

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';
import { showView } from '../sidebar.js';
import { bindTextareaSubmit, setTextareaValue, setupAutosizeTextarea } from '../interaction.js';

let appState = null;
let onNavigateToTranscript = null;

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
  return date.toLocaleDateString('en-US', { weekday: 'short', day: 'numeric', month: 'short' });
}

function formatClockTime(date) {
  return date.toLocaleTimeString('en-US', {
    hour: '2-digit', minute: '2-digit', hour12: false,
  });
}

function groupByDate(items, getTs) {
  const groups = {};
  for (const item of items) {
    const key = formatRelativeDate(getTs(item));
    if (!groups[key]) groups[key] = [];
    groups[key].push(item);
  }
  return groups;
}

export function initMyNotesView(state, callbacks = {}) {
  appState = state;
  onNavigateToTranscript = callbacks.onNavigateToTranscript || null;
}

export async function renderMyNotesView() {
  const container = document.getElementById('view-mynotes');
  if (!container) return;

  let standaloneNotes = [];
  let meetings = [];
  let selectedFolder = null;
  try {
    if (appState.selectedFolderId) {
      const detail = await invoke('get_folder_detail', { folderId: appState.selectedFolderId });
      selectedFolder = detail.folder || null;
      standaloneNotes = detail.standalone_notes || [];
      meetings = detail.meetings || [];
    } else {
      [standaloneNotes, meetings] = await Promise.all([
        invoke('list_standalone_notes'),
        invoke('list_meetings'),
      ]);
    }
  } catch (e) {
    console.error('My notes data load error:', e);
  }

  const allItems = [];

  // Add standalone notes
  for (const n of standaloneNotes) {
    allItems.push({
      type: 'standalone',
      id: n.id,
      title: n.title || 'Untitled',
      ts: n.updated_at,
      meta: (n.text || '').slice(0, 60),
    });
  }

  // Add meeting notes
  const meetingsWithNotes = meetings.filter(m => m.note_count > 0 || m.has_structured_notes);
  for (const m of meetingsWithNotes) {
    allItems.push({
      type: 'meeting',
      id: m.id,
      title: m.title || 'Meeting notes',
      ts: m.started_at,
      meta: `${m.note_count || 0} note${m.note_count === 1 ? '' : 's'}`,
    });
  }

  // Sort by timestamp descending
  allItems.sort((a, b) => (b.ts || 0) - (a.ts || 0));

  const groups = groupByDate(allItems, i => i.ts);

  let listHtml = '';
  for (const [dateLabel, items] of Object.entries(groups)) {
    listHtml += `<div class="meeting-list-date-header">${escapeHtml(dateLabel)}</div>`;
    for (const item of items) {
      const initial = (item.title.trim().charAt(0) || 'N').toUpperCase();
      const date = item.ts ? new Date(item.ts * 1000) : null;
      const timeStr = date ? formatClockTime(date) : '';
      listHtml += `
        <button class="meeting-list-item" data-note-type="${item.type}" data-note-id="${escapeHtml(String(item.id))}">
          <div class="meeting-list-avatar">${escapeHtml(initial)}</div>
          <div class="meeting-list-info">
            <div class="meeting-list-title truncate">${escapeHtml(item.title)}</div>
            <div class="meeting-list-meta">${escapeHtml(item.meta)}</div>
          </div>
          <div class="meeting-list-time">${escapeHtml(timeStr)}</div>
          <div class="meeting-list-lock">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
          </div>
        </button>
      `;
    }
  }

  if (!listHtml) {
    listHtml = `
      <div style="padding: var(--space-8) 0; text-align: center;">
        <p class="text-muted text-sm">${selectedFolder ? 'No notes in this folder yet.' : 'No notes yet. Create a quick note or start a recording session.'}</p>
      </div>
    `;
  }

  const title = selectedFolder?.name || 'My notes';
  const subtitle = selectedFolder?.description || (selectedFolder ? 'Private folder' : 'Your private notes and folders');

  container.innerHTML = `
    <div class="mynotes-view">
      <div class="mynotes-header">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="color:var(--color-text-muted);">
          <rect width="18" height="11" x="3" y="11" rx="2" ry="2"/>
          <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
        </svg>
        <h2 class="mynotes-title">${escapeHtml(title)}</h2>
      </div>
      <p class="mynotes-subtitle">${escapeHtml(subtitle)}</p>

      <div class="mynotes-banner" ${selectedFolder ? 'hidden' : ''}>
        <strong>Your private space</strong> &mdash; Your notes live here by default. Only you can see them unless you share.
      </div>

      <div class="recipe-pills">
        <button class="recipe-pill" data-recipe="weekly_recap">Write weekly recap</button>
        <button class="recipe-pill" data-recipe="recent_todos">List recent todos</button>
        <button class="recipe-pill" data-recipe="summary">Summarize</button>
        <button class="recipe-pill" data-recipe="key_decisions">Key decisions</button>
      </div>

      <hr class="mynotes-divider" />

      ${listHtml}
    </div>
    <div class="view-bottom-bar">
      <textarea class="ask-input prompt-textarea" id="mynotes-ask-input" placeholder="${selectedFolder ? `Ask anything about ${escapeHtml(title)}` : 'Ask anything about My notes'}" rows="1"></textarea>
    </div>
  `;

  // Bind clicks
  container.querySelectorAll('.meeting-list-item').forEach(el => {
    el.addEventListener('click', () => {
      const type = el.dataset.noteType;
      const id = el.dataset.noteId;
      if (type === 'meeting') {
        appState.meetingId = id;
        if (onNavigateToTranscript) onNavigateToTranscript();
      } else if (type === 'standalone') {
        // Navigate to notes view with this note selected
        appState.selectedStandaloneNoteId = parseInt(id);
        showView('notes');
      }
    });
  });

  // Recipe pills
  container.querySelectorAll('[data-recipe]').forEach(btn => {
    btn.addEventListener('click', () => {
      const askInput = document.getElementById('mynotes-ask-input');
      if (askInput) {
        setTextareaValue(askInput, '/' + btn.dataset.recipe, { focus: true, scrollToEnd: true });
      }
    });
  });

  // Ask input
  const askInput = document.getElementById('mynotes-ask-input');
  setupAutosizeTextarea(askInput, { minRows: 1, maxVh: 0.28 });
  bindTextareaSubmit(askInput, async () => {
    const query = askInput.value.trim();
    if (!query) return;
    setTextareaValue(askInput, '', { scrollToEnd: true });
    try {
      askInput.disabled = true;
      askInput.placeholder = 'Thinking...';
      let hasLlm = false;
      try {
        const hasAnthropic = await invoke('has_api_key', { provider: 'anthropic' });
        const hasOpenai = await invoke('has_api_key', { provider: 'openai' });
        const hasGemini = await invoke('has_api_key', { provider: 'gemini' });
        hasLlm = hasAnthropic || hasOpenai || hasGemini;
      } catch { /* ignore */ }
      const command = hasLlm ? 'ask_audire_llm' : 'ask_audire';
      const resp = await invoke(command, {
        query,
        scope: appState.selectedFolderId ? 'folder' : 'all',
        meetingId: null,
        folderId: appState.selectedFolderId || null,
      });
      showToast(resp.answer?.slice(0, 100) || 'Done', 'success');
    } catch (err) {
      showToast('Error: ' + err, 'error');
    } finally {
      askInput.disabled = false;
      askInput.placeholder = selectedFolder ? `Ask anything about ${title}` : 'Ask anything about My notes';
      askInput.focus();
    }
  });
}
