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
  { id: 'gemini', label: 'Google Gemini', desc: 'LLM recipes', placeholder: 'AIza\u2026' },
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

  let profileName = '';
  let profileEmail = '';
  try {
    profileName = localStorage.getItem('audire.user.displayName') || '';
    profileEmail = localStorage.getItem('audire.user.email') || '';
  } catch { /* localStorage unavailable */ }
  const profileInitial = (profileName.trim()[0] || 'A').toUpperCase();
  const profileDisplay = profileName || 'Audire';

  container.innerHTML = `
    <div class="settings-layout">
      <div class="settings-sidebar">
        <div class="settings-profile">
          <div class="settings-profile-avatar">${escapeHtml(profileInitial)}</div>
          <div class="settings-profile-name">${escapeHtml(profileDisplay)}</div>
          <div class="settings-profile-email" style="${profileEmail ? '' : 'display:none;'}">${escapeHtml(profileEmail)}</div>
        </div>

        <div class="settings-nav-section-label">Settings</div>
        <button class="settings-nav-item ${currentSection === 'preferences' ? 'active' : ''}" data-section="preferences">Preferences</button>
        <button class="settings-nav-item ${currentSection === 'calendar' ? 'active' : ''}" data-section="calendar">Calendar</button>
        <button class="settings-nav-item ${currentSection === 'connectors' ? 'active' : ''}" data-section="connectors">API Keys</button>
        <button class="settings-nav-item ${currentSection === 'detection' ? 'active' : ''}" data-section="detection">Detection</button>
        <button class="settings-nav-item ${currentSection === 'ai_provider' ? 'active' : ''}" data-section="ai_provider">AI Provider</button>

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
    case 'detection':
      await renderDetectionSection(panel);
      break;
    case 'ai_provider':
      await renderAiProviderSection(panel);
      break;
    case 'about':
      renderAboutSection(panel);
      break;
  }
}

function renderPreferences(panel) {
  const storedName = (() => {
    try { return localStorage.getItem('audire.user.displayName') || ''; } catch { return ''; }
  })();
  const storedEmail = (() => {
    try { return localStorage.getItem('audire.user.email') || ''; } catch { return ''; }
  })();

  panel.innerHTML = `
    <h2 class="settings-content-title">Preferences</h2>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Display name</div>
        <div class="settings-toggle-desc">Used in greetings and as the speaker label on your transcripts</div>
      </div>
      <input type="text" class="inline-input" id="pref-display-name" value="${escapeHtml(storedName)}" placeholder="Your name" style="min-width:200px;" />
    </div>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Email</div>
        <div class="settings-toggle-desc">Optional. Stored locally for calendar integrations</div>
      </div>
      <input type="email" class="inline-input" id="pref-email" value="${escapeHtml(storedEmail)}" placeholder="you@example.com" style="min-width:200px;" />
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

  const nameInput = document.getElementById('pref-display-name');
  const emailInput = document.getElementById('pref-email');
  nameInput?.addEventListener('change', () => {
    try { localStorage.setItem('audire.user.displayName', nameInput.value.trim()); } catch { /* ignore */ }
    refreshUserCard();
  });
  emailInput?.addEventListener('change', () => {
    try { localStorage.setItem('audire.user.email', emailInput.value.trim()); } catch { /* ignore */ }
    refreshUserCard();
  });

  const themeSelect = document.getElementById('theme-select');
  const storedTheme = (() => {
    try { return localStorage.getItem('audire.theme') || ''; } catch { return ''; }
  })();
  if (storedTheme === 'light') document.documentElement.classList.add('theme-light');
  themeSelect.value = document.documentElement.classList.contains('theme-light') ? 'light' : 'dark';
  themeSelect.addEventListener('change', () => {
    if (themeSelect.value === 'light') {
      document.documentElement.classList.add('theme-light');
    } else {
      document.documentElement.classList.remove('theme-light');
    }
    try { localStorage.setItem('audire.theme', themeSelect.value); } catch { /* ignore */ }
  });
}

function refreshUserCard() {
  let name = '';
  try { name = localStorage.getItem('audire.user.displayName') || ''; } catch { /* ignore */ }
  const cardName = document.querySelector('.user-card-name');
  if (cardName) cardName.textContent = name || 'Audire';
  const cardAvatar = document.querySelector('.user-card .user-avatar');
  if (cardAvatar) cardAvatar.textContent = (name.trim()[0] || 'A').toUpperCase();
  // Also update settings sidebar profile if visible.
  const profName = document.querySelector('.settings-profile-name');
  if (profName) profName.textContent = name || 'Audire';
  const profAvatar = document.querySelector('.settings-profile-avatar');
  if (profAvatar) profAvatar.textContent = (name.trim()[0] || 'A').toUpperCase();
  let email = '';
  try { email = localStorage.getItem('audire.user.email') || ''; } catch { /* ignore */ }
  const profEmail = document.querySelector('.settings-profile-email');
  if (profEmail) {
    profEmail.textContent = email;
    profEmail.style.display = email ? '' : 'none';
  }
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

async function renderDetectionSection(panel) {
  let settings = {};
  try {
    settings = await invoke('get_detection_settings');
  } catch (e) {
    console.error('get_detection_settings error:', e);
  }

  panel.innerHTML = `
    <h2 class="settings-content-title">Detection</h2>
    <p class="settings-section-desc">Automatically detect when meetings start and prompt you to record.</p>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Calendar detection</div>
        <div class="settings-toggle-desc">Prompt to record when a calendar event is about to start</div>
      </div>
      <label class="toggle-switch">
        <input type="checkbox" id="det-calendar-toggle" ${settings.calendar_detection_enabled ? 'checked' : ''} />
        <span class="toggle-slider"></span>
      </label>
    </div>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Lead time</div>
        <div class="settings-toggle-desc">Minutes before event start to show the prompt</div>
      </div>
      <input type="number" class="inline-input" id="det-lead-minutes" min="1" max="30" value="${settings.calendar_lead_minutes || 5}" style="width:70px;" />
    </div>

    <div class="settings-toggle-row">
      <div class="settings-toggle-info">
        <div class="settings-toggle-label">Auto-stop recording</div>
        <div class="settings-toggle-desc">Stop recording when the calendar event ends</div>
      </div>
      <label class="toggle-switch">
        <input type="checkbox" id="det-autostop-toggle" ${settings.auto_stop_enabled ? 'checked' : ''} />
        <span class="toggle-slider"></span>
      </label>
    </div>

  `;

  // Bind toggles
  const calToggle = document.getElementById('det-calendar-toggle');
  const leadInput = document.getElementById('det-lead-minutes');
  const autoStopToggle = document.getElementById('det-autostop-toggle');

  async function saveDetectionSettings() {
    const updated = {
      ...settings,
      calendar_detection_enabled: calToggle.checked,
      calendar_lead_minutes: parseInt(leadInput.value, 10) || 5,
      auto_stop_enabled: autoStopToggle.checked,
    };
    try {
      await invoke('update_detection_settings', { settings: updated });
      settings = updated;
    } catch (e) {
      showToast('Failed to save detection settings: ' + e, 'error');
    }
  }

  calToggle.addEventListener('change', saveDetectionSettings);
  leadInput.addEventListener('change', saveDetectionSettings);
  autoStopToggle.addEventListener('change', saveDetectionSettings);
}

async function renderAiProviderSection(panel) {
  let providers = [];
  let settings = {};
  try {
    providers = await invoke('list_llm_providers');
    settings = await invoke('get_detection_settings');
  } catch (e) {
    console.error('AI provider load error:', e);
  }

  const preferredId = settings.preferred_llm_provider || 'anthropic';

  const providerRowsHtml = providers.map(p => {
    const checked = p.id === preferredId ? 'checked' : '';
    const availClass = p.available ? 'set' : '';
    const availLabel = p.available ? 'Available' : 'Not configured';
    return `
      <div class="key-row">
        <div class="key-row-info" style="align-items:center;">
          <label style="display:flex; align-items:center; gap:var(--space-2); cursor:pointer;">
            <input type="radio" name="preferred-llm" value="${escapeHtml(p.id)}" ${checked} />
            <span class="key-row-label">${escapeHtml(p.name)}</span>
          </label>
          <span class="key-row-status ${availClass}">${availLabel}</span>
        </div>
        <div class="key-row-actions">
          <button class="btn-ghost btn-sm" data-test-llm="${escapeHtml(p.id)}">Test</button>
        </div>
      </div>
    `;
  }).join('');

  const ollamaEndpoint = settings.ollama_endpoint || 'http://localhost:11434';
  const ollamaModel = settings.ollama_model || 'llama3';

  panel.innerHTML = `
    <h2 class="settings-content-title">AI Provider</h2>
    <p class="settings-section-desc">Select which LLM to use for recipes and AI features. Falls back through other available providers if the preferred one fails.</p>

    ${providerRowsHtml}

    <h3 style="margin-top:var(--space-6); margin-bottom:var(--space-3); font-size:var(--text-sm); color:var(--color-text-muted); text-transform:uppercase; letter-spacing:0.04em;">Ollama Configuration</h3>
    <div class="key-row">
      <div class="key-row-info" style="flex-direction:column; align-items:flex-start; gap:var(--space-2);">
        <input type="text" class="inline-input" id="ollama-endpoint" placeholder="http://localhost:11434" value="${escapeHtml(ollamaEndpoint)}" style="width:100%;" />
        <input type="text" class="inline-input" id="ollama-model" placeholder="llama3" value="${escapeHtml(ollamaModel)}" style="width:100%;" />
      </div>
      <div class="key-row-actions">
        <button class="btn-primary btn-sm" id="save-ollama-btn">Save</button>
      </div>
    </div>
  `;

  // Bind preferred provider radio
  panel.querySelectorAll('input[name="preferred-llm"]').forEach(radio => {
    radio.addEventListener('change', async () => {
      try {
        await invoke('set_preferred_llm_provider', { providerId: radio.value });
        showToast('Preferred provider updated', 'success');
      } catch (e) {
        showToast('Failed to set provider: ' + e, 'error');
      }
    });
  });

  // Bind test buttons
  panel.querySelectorAll('[data-test-llm]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const providerId = btn.dataset.testLlm;
      btn.disabled = true;
      btn.textContent = 'Testing\u2026';
      try {
        await invoke('test_llm_provider', { providerId });
        showToast(`${providerId} is working`, 'success');
      } catch (e) {
        showToast(`${providerId} test failed: ${e}`, 'error');
      }
      btn.disabled = false;
      btn.textContent = 'Test';
    });
  });

  // Bind Ollama save
  document.getElementById('save-ollama-btn')?.addEventListener('click', async () => {
    const endpoint = document.getElementById('ollama-endpoint')?.value.trim();
    const model = document.getElementById('ollama-model')?.value.trim() || null;
    if (!endpoint) return;
    try {
      await invoke('save_ollama_endpoint', { endpoint, model });
      showToast('Ollama settings saved', 'success');
    } catch (e) {
      showToast('Failed to save Ollama settings: ' + e, 'error');
    }
  });
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
