// Settings view — two-column layout with sidebar nav

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';

let appState = null;
let currentSection = 'preferences';

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

const API_PROVIDERS = [
  { id: 'deepgram', label: 'Deepgram', desc: 'Flux v2 streaming ASR', placeholder: 'dg_\u2026' },
  { id: 'assemblyai', label: 'AssemblyAI', desc: 'U3 Pro streaming ASR', placeholder: 'Key\u2026' },
  { id: 'openai', label: 'OpenAI', desc: 'LLM recipes', placeholder: 'sk-\u2026' },
  { id: 'anthropic', label: 'Anthropic', desc: 'LLM recipes', placeholder: 'sk-ant-\u2026' },
];

const CALENDAR_PROVIDERS = [
  {
    id: 'google',
    label: 'Google Calendar',
    clientPlaceholder: 'Google OAuth desktop client ID',
    hasSecret: true,
    secretPlaceholder: 'Google OAuth client secret',
    hasTenant: false,
    help: 'Uses Google OAuth for Desktop Apps to read upcoming calendar events.',
  },
  {
    id: 'microsoft',
    label: 'Microsoft Outlook',
    clientPlaceholder: 'Microsoft Entra application (client) ID',
    hasTenant: true,
    tenantPlaceholder: 'Tenant ID or common',
    help: 'Uses Microsoft identity platform OAuth to read Outlook calendar events.',
  },
];

export function initSettingsView(state) {
  appState = state;
}

export async function renderSettingsView() {
  const container = document.getElementById('view-settings');
  if (!container) return;

  container.innerHTML = `
    <div class="settings-layout">
      <div class="settings-sidebar">
        <div class="settings-profile">
          <div class="settings-profile-avatar">A</div>
          <div class="settings-profile-name">Audire User</div>
          <div class="settings-profile-email">user@audire.app</div>
        </div>

        <div class="settings-nav-section-label">Settings</div>
        <button class="settings-nav-item ${currentSection === 'preferences' ? 'active' : ''}" data-section="preferences">Preferences</button>
        <button class="settings-nav-item ${currentSection === 'calendar' ? 'active' : ''}" data-section="calendar">Calendar</button>
        <button class="settings-nav-item ${currentSection === 'connectors' ? 'active' : ''}" data-section="connectors">API Keys</button>

        <div class="settings-nav-section-label">Info</div>
        <button class="settings-nav-item ${currentSection === 'about' ? 'active' : ''}" data-section="about">About</button>
      </div>

      <div class="settings-content" id="settings-content-panel">
        <!-- Content rendered by section -->
      </div>
    </div>
  `;

  // Bind sidebar nav
  container.querySelectorAll('[data-section]').forEach(btn => {
    btn.addEventListener('click', () => {
      currentSection = btn.dataset.section;
      container.querySelectorAll('.settings-nav-item').forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      renderSection();
    });
  });

  await renderSection();
}

async function renderSection() {
  const panel = document.getElementById('settings-content-panel');
  if (!panel) return;

  switch (currentSection) {
    case 'preferences':
      renderPreferences(panel);
      break;
    case 'calendar':
      await renderCalendarSection(panel);
      break;
    case 'connectors':
      await renderConnectorsSection(panel);
      break;
    case 'about':
      renderAboutSection(panel);
      break;
  }
}

function renderPreferences(panel) {
  panel.innerHTML = `
    <h2 class="settings-content-title">Preferences</h2>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Live meeting indicator</div>
        <div class="settings-toggle-desc">Show a visual indicator when a meeting is being transcribed</div>
      </div>
      <label class="toggle-switch">
        <input type="checkbox" checked />
        <span class="toggle-slider"></span>
      </label>
    </div>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Open on login</div>
        <div class="settings-toggle-desc">Automatically start Audire when you log in</div>
      </div>
      <label class="toggle-switch">
        <input type="checkbox" />
        <span class="toggle-slider"></span>
      </label>
    </div>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Move aside when not in use</div>
        <div class="settings-toggle-desc">Minimize the window when not actively transcribing</div>
      </div>
      <label class="toggle-switch">
        <input type="checkbox" />
        <span class="toggle-slider"></span>
      </label>
    </div>

    <div class="settings-toggle-row" style="border-bottom:none;">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Theme</div>
        <div class="settings-toggle-desc">Choose your preferred appearance</div>
      </div>
      <select class="settings-select" id="theme-select">
        <option value="dark">Dark</option>
        <option value="light">Light</option>
      </select>
    </div>
  `;

  // Theme toggle
  const themeSelect = document.getElementById('theme-select');
  themeSelect.value = document.documentElement.classList.contains('theme-light') ? 'light' : 'dark';
  themeSelect.addEventListener('change', () => {
    if (themeSelect.value === 'light') {
      document.documentElement.classList.add('theme-light');
    } else {
      document.documentElement.classList.remove('theme-light');
    }
  });
}

// Track connection errors per provider so they persist across re-renders
const calendarErrors = {};

async function renderCalendarSection(panel) {
  let calendarStatuses = [];
  try {
    calendarStatuses = await invoke('list_calendar_statuses');
  } catch (e) {
    console.error('list_calendar_statuses error:', e);
  }

  const calendarRowsHtml = CALENDAR_PROVIDERS.map(prov => {
    const status = calendarStatuses.find(s => s.provider === prov.id) || {};
    const statusLabel = status.connected ? 'Connected' : status.configured ? 'Configured' : 'Not set';
    const statusClass = status.connected ? 'connected' : 'not-set';
    const lastError = calendarErrors[prov.id] || '';
    return `
      <div class="calendar-row">
        <div class="calendar-provider-header">
          <span class="calendar-provider-label">${escapeHtml(prov.label)}</span>
          <span class="calendar-provider-status ${statusClass}">${escapeHtml(statusLabel)}</span>
        </div>
        <div class="calendar-help">${escapeHtml(prov.help)}</div>
        ${status.email ? `<div class="calendar-connected-as">Connected as ${escapeHtml(status.email)}</div>` : ''}
        ${lastError ? `<div class="calendar-error">${escapeHtml(lastError)}</div>` : ''}
        <div class="calendar-fields">
          <input type="text" class="inline-input" style="flex:1;" id="cal-client-${prov.id}" placeholder="${escapeHtml(prov.clientPlaceholder)}" value="${escapeHtml(status.client_id || '')}" />
          ${prov.hasSecret ? `<input type="password" class="inline-input" style="flex:1;" id="cal-secret-${prov.id}" placeholder="${escapeHtml(prov.secretPlaceholder)}" value="${escapeHtml(status.client_secret || '')}" />` : ''}
          ${prov.hasTenant ? `<input type="text" class="inline-input" style="flex:1;" id="cal-tenant-${prov.id}" placeholder="${escapeHtml(prov.tenantPlaceholder)}" value="${escapeHtml(status.tenant_id || '')}" />` : ''}
        </div>
        <div class="calendar-actions">
          <button class="btn-primary btn-sm" data-save-cal="${prov.id}">Save config</button>
          <button class="btn-ghost btn-sm" data-connect-cal="${prov.id}" ${status.configured ? '' : 'disabled'}>
            ${status.connected ? 'Reconnect' : 'Connect'}
          </button>
          ${status.connected || status.configured ? `<button class="btn-ghost btn-sm btn-danger" data-disconnect-cal="${prov.id}">Disconnect</button>` : ''}
        </div>
      </div>
    `;
  }).join('');

  panel.innerHTML = `
    <h2 class="settings-content-title">Calendar</h2>
    <p class="settings-section-desc">Connect your calendar to see upcoming meetings on the home screen.</p>
    ${calendarRowsHtml}
  `;

  bindCalendarEvents();
}

async function renderConnectorsSection(panel) {
  const keyStatuses = {};
  const keySources = {};
  for (const p of API_PROVIDERS) {
    try {
      keyStatuses[p.id] = await invoke('has_api_key', { provider: p.id });
    } catch {
      keyStatuses[p.id] = false;
    }
    try {
      const resolution = await invoke('resolve_provider_key_source', { provider: p.id, orgId: null });
      keySources[p.id] = resolution?.source ? resolution.source.replaceAll('_', ' ') : '';
    } catch {
      keySources[p.id] = keyStatuses[p.id] ? 'Available' : '';
    }
  }

  const keyRowsHtml = API_PROVIDERS.map(p => {
    const hasKey = keyStatuses[p.id];
    const source = keySources[p.id];
    return `
      <div class="key-row">
        <div class="key-row-info">
          <span class="key-row-label">${escapeHtml(p.label)}</span>
          <span class="key-row-status ${hasKey ? 'set' : ''}" id="status-${p.id}">
            ${hasKey ? '\u25CF Set' : 'Not set'}
          </span>
          ${source ? `<span class="key-row-source">${escapeHtml(source)}</span>` : ''}
        </div>
        <div class="key-row-actions">
          <input
            type="password"
            class="key-input"
            placeholder="${escapeHtml(p.placeholder)}"
            id="key-input-${p.id}"
            autocomplete="off"
          />
          <button class="btn-primary btn-sm" data-save-key="${p.id}">Save</button>
          ${hasKey ? `<button class="btn-ghost btn-sm btn-danger" data-delete-key="${p.id}">Delete</button>` : ''}
        </div>
      </div>
    `;
  }).join('');

  panel.innerHTML = `
    <h2 class="settings-content-title">API Keys</h2>
    <p class="settings-section-desc">
      Keys are stored in your OS keyring (macOS Keychain / Windows Credential Manager /
      Linux Secret Service). They never leave the native layer.
    </p>
    ${keyRowsHtml}
  `;

  bindKeyEvents();
}

function renderAboutSection(panel) {
  panel.innerHTML = `
    <h2 class="settings-content-title">About</h2>
    <div class="settings-info-card">
      <p>
        <strong>Audire</strong> &mdash; local-first meeting transcription.
      </p>
      <p style="margin-top: var(--space-3);">
        Privacy-first: no audio written to disk, BYOK keys, encrypted DB (SQLCipher).
      </p>
    </div>
  `;
}

function bindKeyEvents() {
  // Save API keys
  document.querySelectorAll('[data-save-key]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.saveKey;
      const input = document.getElementById(`key-input-${provider}`);
      const key = input?.value.trim();
      if (!key) return;
      try {
        await invoke('save_api_key', { provider, key });
        input.value = '';
        showToast(`${provider} key saved`, 'success');
        await renderConnectorsSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to save key: ' + e, 'error');
      }
    });
  });

  // Enter key on inputs triggers save
  document.querySelectorAll('.key-input').forEach(input => {
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        const row = input.closest('.key-row');
        row?.querySelector('[data-save-key]')?.click();
      }
    });
  });

  // Delete API keys
  document.querySelectorAll('[data-delete-key]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.deleteKey;
      try {
        await invoke('delete_api_key', { provider });
        showToast(`${provider} key deleted`, 'success');
        await renderConnectorsSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to delete key: ' + e, 'error');
      }
    });
  });
}

function bindCalendarEvents() {
  // Save calendar config
  document.querySelectorAll('[data-save-cal]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.saveCal;
      const clientId = document.getElementById(`cal-client-${provider}`)?.value.trim();
      const clientSecret = document.getElementById(`cal-secret-${provider}`)?.value.trim() || null;
      const tenantId = document.getElementById(`cal-tenant-${provider}`)?.value.trim() || null;
      if (!clientId) return;
      try {
        await invoke('save_calendar_config', { provider, clientId, clientSecret, tenantId });
        delete calendarErrors[provider];
        showToast(`${provider} calendar config saved`, 'success');
        await renderCalendarSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to save calendar config: ' + e, 'error');
      }
    });
  });

  // Connect calendar
  document.querySelectorAll('[data-connect-cal]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.connectCal;
      btn.disabled = true;
      btn.textContent = 'Connecting\u2026';
      try {
        await invoke('connect_calendar_provider', { provider });
        delete calendarErrors[provider];
        showToast(`${provider} calendar connected`, 'success');
        await renderCalendarSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        const errMsg = String(e);
        calendarErrors[provider] = 'Connection failed: ' + errMsg;
        showToast('Failed to connect: ' + errMsg, 'error');
        await renderCalendarSection(document.getElementById('settings-content-panel'));
      }
    });
  });

  // Disconnect calendar
  document.querySelectorAll('[data-disconnect-cal]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const provider = btn.dataset.disconnectCal;
      try {
        await invoke('disconnect_calendar_provider', { provider });
        delete calendarErrors[provider];
        showToast(`${provider} calendar disconnected`, 'success');
        await renderCalendarSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to disconnect: ' + e, 'error');
      }
    });
  });
}
