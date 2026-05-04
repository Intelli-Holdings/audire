// People view — data table with inline add form

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';

let appState = null;

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

function formatRelativeDate(tsSeconds) {
  if (!tsSeconds) return 'No notes';
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

function getInitials(name) {
  const parts = (name || '').split(/\s+/).filter(Boolean);
  return ((parts[0]?.[0] || '?') + (parts[1]?.[0] || '')).toUpperCase().slice(0, 2);
}

export function initPeopleView(state) {
  appState = state;
}

export async function renderPeopleView() {
  const container = document.getElementById('view-people');
  if (!container) return;

  let people = [];
  try {
    people = await invoke('list_all_participants');
  } catch (e) {
    console.error('list_all_participants error:', e);
  }

  const sorted = [...people].sort((a, b) => (b.last_meeting_at || 0) - (a.last_meeting_at || 0));

  let tableHtml = '';
  if (sorted.length > 0) {
    const rows = sorted.map(p => `
      <tr>
        <td>
          <div class="person-avatar-cell">
            <div class="person-avatar-sm">${escapeHtml(getInitials(p.name))}</div>
            <div>
              <div style="font-weight:var(--weight-medium)">${escapeHtml(p.name)}</div>
              <div class="text-xs text-muted">${escapeHtml(p.email || p.org_name || '')}</div>
            </div>
          </div>
        </td>
        <td class="text-muted">${escapeHtml(formatRelativeDate(p.last_meeting_at))}</td>
        <td>${p.meeting_count || 0}</td>
      </tr>
    `).join('');

    tableHtml = `
      <div class="data-table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th>Person</th>
              <th>Last note</th>
              <th>Notes</th>
            </tr>
          </thead>
          <tbody>${rows}</tbody>
        </table>
      </div>
    `;
  }

  container.innerHTML = `
    <div class="data-view">
      <div class="data-view-header">
        <h1 class="data-view-title">People</h1>
        <button class="btn-ghost btn-sm" id="toggle-add-person" type="button" aria-expanded="false" aria-controls="add-person-form">+ Add person</button>
      </div>

      <div class="inline-add-form" id="add-person-form" hidden>
        <input class="inline-input" placeholder="Name" id="new-person-name" style="flex:1;" autocomplete="name" />
        <input class="inline-input" placeholder="Email (optional)" id="new-person-email" style="flex:1;" />
        <button class="btn-primary btn-sm" id="save-person-btn" type="button" disabled>Add</button>
        <button class="btn-ghost btn-sm" id="cancel-person-btn" type="button">Cancel</button>
      </div>

      ${tableHtml || `
        <div class="empty-state">
          <p class="empty-state-title">No people yet</p>
          <p class="empty-state-body">People from your meetings will appear here, or add them manually.</p>
        </div>
      `}
    </div>
  `;

  // Toggle add form
  const addForm = document.getElementById('add-person-form');
  const toggleBtn = document.getElementById('toggle-add-person');
  const nameInput = document.getElementById('new-person-name');
  const emailInput = document.getElementById('new-person-email');
  const saveBtn = document.getElementById('save-person-btn');

  function closeForm() {
    addForm.hidden = true;
    toggleBtn?.setAttribute('aria-expanded', 'false');
    if (nameInput) nameInput.value = '';
    if (emailInput) emailInput.value = '';
    if (saveBtn) saveBtn.disabled = true;
  }

  toggleBtn?.addEventListener('click', () => {
    addForm.hidden = !addForm.hidden;
    toggleBtn.setAttribute('aria-expanded', String(!addForm.hidden));
    if (!addForm.hidden) nameInput?.focus();
  });
  document.getElementById('cancel-person-btn')?.addEventListener('click', closeForm);
  nameInput?.addEventListener('input', () => {
    if (saveBtn) saveBtn.disabled = !nameInput.value.trim();
  });
  addForm?.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeForm();
    if (e.key === 'Enter') {
      e.preventDefault();
      saveBtn?.click();
    }
  });

  // Save person
  document.getElementById('save-person-btn')?.addEventListener('click', async () => {
    const name = document.getElementById('new-person-name')?.value.trim();
    if (!name) return;
    const email = document.getElementById('new-person-email')?.value.trim() || null;
    try {
      saveBtn.disabled = true;
      saveBtn.textContent = 'Adding...';
      await invoke('add_participant', { meetingId: null, name, email });
      showToast('Person added', 'success');
      await renderPeopleView();
    } catch (e) {
      showToast('Failed to add person: ' + e, 'error');
    } finally {
      saveBtn.disabled = !document.getElementById('new-person-name')?.value.trim();
      saveBtn.textContent = 'Add';
    }
  });
}
