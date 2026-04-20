// Companies view — data table with inline add form

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

function getInitials(label) {
  const clean = (label || '').replace(/^www\./i, '');
  const parts = clean.split(/[.\s-]+/).filter(Boolean);
  return ((parts[0]?.[0] || '?') + (parts[1]?.[0] || '')).toUpperCase().slice(0, 2);
}

export function initCompaniesView(state) {
  appState = state;
}

export async function renderCompaniesView() {
  const container = document.getElementById('view-companies');
  if (!container) return;

  let orgs = [];
  try {
    orgs = await invoke('list_organizations');
  } catch (e) {
    console.error('list_organizations error:', e);
  }

  const sorted = [...orgs].sort((a, b) => (b.last_meeting_at || 0) - (a.last_meeting_at || 0));

  let tableHtml = '';
  if (sorted.length > 0) {
    const rows = sorted.map(org => {
      const displayName = org.domain || org.name;
      return `
        <tr>
          <td>
            <div class="person-avatar-cell">
              <div class="person-avatar-sm">${escapeHtml(getInitials(displayName))}</div>
              <div>
                <div style="font-weight:var(--weight-medium)">${escapeHtml(displayName)}</div>
                <div class="text-xs text-muted">${escapeHtml(org.domain || '')}</div>
              </div>
            </div>
          </td>
          <td class="text-muted">${escapeHtml(formatRelativeDate(org.last_meeting_at))}</td>
          <td>${Math.max(1, org.people_count || 0)}</td>
        </tr>
      `;
    }).join('');

    tableHtml = `
      <div class="data-table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th>Company</th>
              <th>Last note</th>
              <th>People</th>
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
        <h1 class="data-view-title">Companies</h1>
        <button class="btn-ghost btn-sm" id="toggle-add-company">+ Add company</button>
      </div>

      <div class="inline-add-form" id="add-company-form" hidden>
        <input class="inline-input" placeholder="Company name" id="new-org-name" style="flex:1;" />
        <input class="inline-input" placeholder="Domain (optional)" id="new-org-domain" style="flex:1;" />
        <button class="btn-primary btn-sm" id="save-org-btn">Add</button>
        <button class="btn-ghost btn-sm" id="cancel-org-btn">Cancel</button>
      </div>

      ${tableHtml || `
        <div class="empty-state">
          <p class="empty-state-title">No companies yet</p>
          <p class="empty-state-body">Companies of people you meet will appear here, or add them manually.</p>
        </div>
      `}
    </div>
  `;

  // Toggle add form
  const addForm = document.getElementById('add-company-form');
  document.getElementById('toggle-add-company')?.addEventListener('click', () => {
    addForm.hidden = !addForm.hidden;
    if (!addForm.hidden) document.getElementById('new-org-name')?.focus();
  });
  document.getElementById('cancel-org-btn')?.addEventListener('click', () => {
    addForm.hidden = true;
  });

  // Save company
  document.getElementById('save-org-btn')?.addEventListener('click', async () => {
    const name = document.getElementById('new-org-name')?.value.trim();
    if (!name) return;
    const domain = document.getElementById('new-org-domain')?.value.trim() || null;
    try {
      await invoke('add_organization', { name, domain });
      showToast('Company added', 'success');
      await renderCompaniesView();
    } catch (e) {
      showToast('Failed to add company: ' + e, 'error');
    }
  });
}
