// Settings view — two-column layout with sidebar nav

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';
import { trapDialogFocus } from '../interaction.js';

let appState = null;
let currentSection = 'preferences';

/**
 * Programmatically pre-select a section before the next render. Used by other
 * views (e.g. the BYOK pre-flight in sidebar.js) to deep-link into a specific
 * settings panel without hardcoding the URL hash scheme.
 */
export function setSettingsSection(name) {
  currentSection = name;
}

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

const API_PROVIDERS = [
  {
    id: 'deepgram',
    label: 'Deepgram',
    desc: 'Flux v2 streaming ASR',
    placeholder: 'dg_\u2026',
    kind: 'asr',
    signupUrl: 'https://console.deepgram.com/signup',
  },
  {
    id: 'assemblyai',
    label: 'AssemblyAI',
    desc: 'U3 Pro streaming ASR',
    placeholder: 'Key\u2026',
    kind: 'asr',
    signupUrl: 'https://www.assemblyai.com/dashboard/signup',
  },
  {
    id: 'openai',
    label: 'OpenAI',
    desc: 'LLM recipes',
    placeholder: 'sk-\u2026',
    kind: 'llm',
    signupUrl: 'https://platform.openai.com/api-keys',
  },
  {
    id: 'anthropic',
    label: 'Anthropic',
    desc: 'LLM recipes',
    placeholder: 'sk-ant-\u2026',
    kind: 'llm',
    signupUrl: 'https://console.anthropic.com/settings/keys',
  },
  {
    id: 'gemini',
    label: 'Google Gemini',
    desc: 'LLM recipes',
    placeholder: 'AIza\u2026',
    kind: 'llm',
    signupUrl: 'https://aistudio.google.com/apikey',
  },
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

        <div class="settings-nav-section-label">Sync</div>
        <button class="settings-nav-item ${currentSection === 'account' ? 'active' : ''}" data-section="account">Account</button>

        <div class="settings-nav-section-label">Info</div>
        <button class="settings-nav-item ${currentSection === 'privacy' ? 'active' : ''}" data-section="privacy">Privacy</button>
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
    case 'account':
      await renderAccountSection(panel);
      break;
    case 'privacy':
      renderPrivacySection(panel);
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

  const renderRow = (p) => {
    const hasKey = keyStatuses[p.id];
    const source = keySources[p.id];
    return `
      <div class="key-row">
        <div class="key-row-info">
          <span class="key-row-label">${escapeHtml(p.label)}</span>
          <span class="key-row-desc">${escapeHtml(p.desc)}</span>
          <span class="key-row-status ${hasKey ? 'set' : ''}" id="status-${p.id}">
            ${hasKey ? '\u25CF Set' : 'Not set'}
          </span>
          ${source ? `<span class="key-row-source">${escapeHtml(source)}</span>` : ''}
          ${!hasKey && p.signupUrl ? `<a class="key-row-signup" href="${escapeHtml(p.signupUrl)}" target="_blank" rel="noopener">Get a key \u2192</a>` : ''}
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
  };

  const asrProviders = API_PROVIDERS.filter((p) => p.kind === 'asr');
  const llmProviders = API_PROVIDERS.filter((p) => p.kind === 'llm');
  const hasAnyAsr = asrProviders.some((p) => keyStatuses[p.id]);
  const hasAnyLlm = llmProviders.some((p) => keyStatuses[p.id]);

  // First-run empty state \u2014 shown when the user has no keys at all. Explains
  // BYOK in plain terms and links to provider signup pages so the path from
  // "I just installed Audire" to "I have a transcript" is unambiguous.
  const emptyStateHtml = (!hasAnyAsr && !hasAnyLlm) ? `
    <div class="byok-empty-state">
      <h3>Bring your own keys to get started</h3>
      <p>
        Audire never charges for AI usage. Transcription and language-model
        features run on your own provider account using your own API key, so
        your audio and prompts go straight from this device to the provider
        you choose &mdash; never through our servers.
      </p>
      <p>
        You'll need at least one transcription key (Deepgram or AssemblyAI)
        before you can record. Pick whichever you prefer; both have free tiers.
      </p>
    </div>
  ` : '';

  panel.innerHTML = `
    <h2 class="settings-content-title">API Keys</h2>
    <p class="settings-section-desc">
      Keys are stored in your OS keyring (macOS Keychain / Windows Credential Manager /
      Linux Secret Service). They never leave the native layer and are never
      returned to the WebView.
    </p>
    ${emptyStateHtml}
    <h3 class="settings-subsection-title">
      Transcription
      <span class="settings-subsection-hint">${hasAnyAsr ? 'Required to record &middot; you have at least one key' : 'Required to record'}</span>
    </h3>
    ${asrProviders.map(renderRow).join('')}
    <h3 class="settings-subsection-title" style="margin-top: var(--space-5);">
      Language models
      <span class="settings-subsection-hint">${hasAnyLlm ? 'Optional &middot; powers AI recipes' : 'Optional &middot; powers AI recipes'}</span>
    </h3>
    ${llmProviders.map(renderRow).join('')}
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

    <h3 style="margin-top:var(--space-6); margin-bottom:var(--space-3); font-size:var(--text-sm); color:var(--color-text-muted); text-transform:uppercase; letter-spacing:0;">Ollama Configuration</h3>
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
        Audio is never persisted, anywhere. Transcripts and notes live in an
        encrypted local database. Cloud providers are called only with the
        keys you supply, and only over TLS.
      </p>
      <p style="margin-top: var(--space-3); color: var(--color-text-secondary); font-size: var(--text-xs);">
        See <strong>Privacy</strong> in the sidebar for the full guarantees.
      </p>
    </div>
  `;
}

async function renderAccountSection(panel) {
  let status = null;
  try {
    status = await invoke('sync_account_status');
  } catch (e) {
    panel.innerHTML = `<h2 class="settings-content-title">Account</h2>
      <div class="settings-info-card"><p>Failed to read account status: ${escapeHtml(String(e))}</p></div>`;
    return;
  }

  if (status.mode !== 'cloud') {
    renderAccountSignedOut(panel);
    return;
  }

  let orgs = [];
  let vaults = [];
  let runningVaults = [];
  try {
    orgs = await invoke('sync_list_orgs');
    vaults = await invoke('sync_list_vaults');
    runningVaults = await invoke('sync_running_vaults');
  } catch { /* table reads can be empty before unlock */ }

  panel.innerHTML = `
    <h2 class="settings-content-title">Account</h2>
    <p class="settings-section-desc">
      Audire Sync is end-to-end encrypted. The server only stores ciphertext &mdash;
      it can't read your notes, and audio is never uploaded.
    </p>

    <div class="settings-info-card">
      <div style="display:flex; justify-content:space-between; align-items:center;">
        <div>
          <div style="font-weight:600;">${escapeHtml(status.email || '')}</div>
          <div style="font-size:var(--text-xs); color:var(--color-text-secondary);">
            Server: ${escapeHtml(status.server_url || '')}
          </div>
          <div style="font-size:var(--text-xs); color:var(--color-text-secondary);">
            User ID: <code>${escapeHtml(status.user_id || '')}</code>
          </div>
        </div>
        <button class="btn btn-secondary" id="sync-sign-out">Sign out</button>
      </div>
    </div>

    <div id="sync-unlock-row" class="settings-info-card" style="margin-top:var(--space-3); display:none;">
      <h3 style="margin:0 0 var(--space-2) 0;">Unlock</h3>
      <p style="color:var(--color-text-secondary); font-size:var(--text-xs);">
        Re-enter your passphrase to unlock vault keys for this session.
      </p>
      <div style="display:flex; gap:var(--space-2); margin-top:var(--space-2);">
        <input type="password" class="inline-input" id="sync-unlock-input" placeholder="Passphrase" style="flex:1;" />
        <button class="btn btn-primary" id="sync-unlock-btn">Unlock</button>
      </div>
    </div>

    <div class="settings-info-card" style="margin-top:var(--space-3);">
      <div style="display:flex; justify-content:space-between; align-items:center;">
        <h3 style="margin:0;">Organizations</h3>
        <button class="btn btn-secondary" id="sync-create-org">New organization</button>
      </div>
      <div id="sync-orgs-list" style="margin-top:var(--space-2);">
        ${orgs.length === 0
          ? '<p style="color:var(--color-text-secondary); font-size:var(--text-xs);">You aren\'t in any organizations yet. Create one to share notes with your team.</p>'
          : orgs.map(o => `
            <div class="org-row" data-org-id="${escapeHtml(o.org_id)}" style="display:flex; justify-content:space-between; align-items:center; padding:var(--space-2) 0; border-bottom:1px solid var(--color-border);">
              <div>
                <div style="font-weight:600;">${escapeHtml(o.name)}</div>
                <div style="font-size:var(--text-xs); color:var(--color-text-secondary);">Role: ${escapeHtml(o.role)}</div>
              </div>
              <div style="display:flex; gap:var(--space-2);">
                <input type="email" class="inline-input invite-email" placeholder="Invite by email" style="min-width:220px;" />
                <button class="btn btn-secondary" data-invite-org="${escapeHtml(o.org_id)}">Invite</button>
              </div>
            </div>`).join('')}
      </div>
    </div>

    <div class="settings-info-card" style="margin-top:var(--space-3);">
      <div style="display:flex; justify-content:space-between; align-items:center;">
        <h3 style="margin:0;">Sync status</h3>
        <button class="btn btn-secondary" id="sync-refresh-btn">Refresh</button>
      </div>
      <div id="sync-vaults-list" style="margin-top:var(--space-2);">
        ${vaults.length === 0
          ? '<p style="color:var(--color-text-secondary); font-size:var(--text-xs);">No shared folders yet.</p>'
          : vaults.map(v => {
              const isRunning = runningVaults.includes(v.vault_id);
              return `<div style="padding:var(--space-2) 0; border-bottom:1px solid var(--color-border);">
                <div style="display:flex; justify-content:space-between; align-items:center;">
                  <div>
                    <div style="font-weight:600;">${escapeHtml(v.name || 'Vault ' + v.vault_id.slice(0,8))}</div>
                    <div style="font-size:var(--text-xs); color:var(--color-text-secondary);">
                      Cursor: ${v.last_op_id_applied} / ${v.last_op_id_remote}
                    </div>
                  </div>
                  <span class="sync-status-pill" data-vault="${escapeHtml(v.vault_id)}"
                    style="font-size:var(--text-xs); padding: 2px 8px; border-radius:999px; background:${isRunning ? 'var(--color-accent-soft)' : 'var(--color-surface-2)'};">
                    ${isRunning ? 'live' : 'idle'}
                  </span>
                </div>
              </div>`;
            }).join('')}
      </div>
    </div>
  `;

  bindAccountEvents(panel, status);

  // Subscribe to live sync events to update the status pill.
  try {
    const { listen } = await import('@tauri-apps/api/event');
    if (!window.__audireSyncListenerInstalled) {
      window.__audireSyncListenerInstalled = true;
      listen('audire://sync_status', (ev) => {
        const pill = document.querySelector(`.sync-status-pill[data-vault="${ev.payload.vault_id}"]`);
        if (pill) {
          pill.textContent = ev.payload.state;
        }
      });
    }
  } catch { /* event API may not be available in non-Tauri builds */ }
}

function renderAccountSignedOut(panel) {
  panel.innerHTML = `
    <h2 class="settings-content-title">Account</h2>
    <p class="settings-section-desc">
      Audire Sync is optional. Sign in to share notes with an organization. Your
      audio is never uploaded &mdash; only encrypted notes and folder metadata.
    </p>

    <div class="settings-info-card">
      <h3 style="margin:0 0 var(--space-2) 0;">Sign up</h3>
      <p style="color:var(--color-text-secondary); font-size:var(--text-xs);">
        Connect this device to a sync server. If you forget your passphrase, the
        recovery key shown after sign-up is the only way to recover access.
      </p>

      <div style="display:flex; flex-direction:column; gap:var(--space-2); margin-top:var(--space-2);">
        <input type="text" class="inline-input" id="sync-server-url" placeholder="https://audire-server.fly.dev" />
        <input type="email" class="inline-input" id="sync-email" placeholder="you@example.com" />
        <input type="text" class="inline-input" id="sync-access-token" placeholder="Access token from the sign-in flow" />
        <input type="password" class="inline-input" id="sync-passphrase" placeholder="Passphrase (used for end-to-end encryption)" />
        <input type="password" class="inline-input" id="sync-passphrase-confirm" placeholder="Confirm passphrase" />
        <div style="display:flex; gap:var(--space-2); justify-content:flex-end;">
          <button class="btn btn-primary" id="sync-sign-up-btn">Create account</button>
        </div>
      </div>
    </div>

    <div class="settings-info-card" style="margin-top:var(--space-3);">
      <h3 style="margin:0 0 var(--space-2) 0;">Already have an account?</h3>
      <p style="color:var(--color-text-secondary); font-size:var(--text-xs);">
        Multi-device sign-in arrives in the next desktop release. For now, sign
        up on this device and use your recovery key to bring data over.
      </p>
    </div>
  `;

  document.getElementById('sync-sign-up-btn')?.addEventListener('click', async () => {
    const server_url = document.getElementById('sync-server-url').value.trim();
    const email = document.getElementById('sync-email').value.trim();
    const access_token = document.getElementById('sync-access-token').value.trim();
    const passphrase = document.getElementById('sync-passphrase').value;
    const confirm = document.getElementById('sync-passphrase-confirm').value;
    if (!server_url || !email || !access_token || !passphrase) {
      showToast('Fill in every field', 'error');
      return;
    }
    if (passphrase !== confirm) {
      showToast('Passphrases do not match', 'error');
      return;
    }
    try {
      const { recovery_hex } = await invoke('sync_sign_up', {
        request: { server_url, email, access_token, passphrase },
      });
      // Show the recovery key once. After this point, the user is on
      // their own — we never store the recovery key in plaintext.
      const dialog = document.createElement('div');
      dialog.className = 'modal-backdrop';
      dialog.innerHTML = `
        <div class="modal" role="dialog" aria-modal="true" aria-labelledby="recovery-title" aria-describedby="recovery-desc" style="max-width:520px;">
          <h3 id="recovery-title">Save your recovery key</h3>
          <p id="recovery-desc">This is shown <strong>once</strong>. If you forget your passphrase, this is the only way to recover access to your shared notes. Store it offline.</p>
          <pre id="recovery-hex" style="background:var(--color-surface-2); padding:var(--space-3); border-radius:var(--radius-md); user-select:all;">${escapeHtml(recovery_hex)}</pre>
          <div style="display:flex; gap:var(--space-2); justify-content:flex-end;">
            <button class="btn btn-secondary" id="copy-recovery">Copy</button>
            <button class="btn btn-primary" id="dismiss-recovery">I have saved it</button>
          </div>
        </div>`;
      document.body.appendChild(dialog);
      let releaseDialogFocus = null;
      const closeRecoveryDialog = () => {
        releaseDialogFocus?.();
        document.body.removeChild(dialog);
        renderAccountSection(document.getElementById('settings-content-panel'));
      };
      releaseDialogFocus = trapDialogFocus(dialog, {
        initialFocus: dialog.querySelector('#copy-recovery'),
      });
      dialog.querySelector('#copy-recovery').addEventListener('click', () => {
        navigator.clipboard.writeText(recovery_hex);
        showToast('Recovery key copied', 'success');
      });
      dialog.querySelector('#dismiss-recovery').addEventListener('click', () => {
        closeRecoveryDialog();
      });
      showToast('Account created', 'success');
    } catch (e) {
      showToast('Sign up failed: ' + e, 'error');
    }
  });
}

function bindAccountEvents(panel, status) {
  document.getElementById('sync-sign-out')?.addEventListener('click', async () => {
    if (!confirm('Sign out and remove this device from sync? Your local notes stay intact.')) return;
    try {
      await invoke('sync_sign_out');
      showToast('Signed out', 'success');
      await renderAccountSection(panel);
    } catch (e) {
      showToast('Sign out failed: ' + e, 'error');
    }
  });

  document.getElementById('sync-create-org')?.addEventListener('click', async () => {
    const name = prompt('Organization name');
    if (!name) return;
    try {
      await invoke('sync_create_org', { args: { name } });
      showToast(`Created organization "${name}"`, 'success');
      await renderAccountSection(panel);
    } catch (e) {
      if (String(e).includes('locked')) {
        document.getElementById('sync-unlock-row').style.display = '';
        showToast('Unlock with your passphrase first', 'error');
      } else {
        showToast('Create org failed: ' + e, 'error');
      }
    }
  });

  document.querySelectorAll('[data-invite-org]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const orgId = btn.dataset.inviteOrg;
      const row = btn.closest('.org-row');
      const email = row?.querySelector('.invite-email')?.value.trim();
      if (!email) return;
      try {
        const result = await invoke('sync_invite_to_org', {
          args: { org_id: orgId, email, org_role: 'member', vault_role: 'editor' },
        });
        showToast(`Invited ${result.email}`, 'success');
        if (row.querySelector('.invite-email')) row.querySelector('.invite-email').value = '';
      } catch (e) {
        if (String(e).includes('locked')) {
          document.getElementById('sync-unlock-row').style.display = '';
          showToast('Unlock with your passphrase first', 'error');
        } else {
          showToast('Invite failed: ' + e, 'error');
        }
      }
    });
  });

  document.getElementById('sync-unlock-btn')?.addEventListener('click', async () => {
    const passphrase = document.getElementById('sync-unlock-input').value;
    if (!passphrase) return;
    try {
      await invoke('sync_unlock', { passphrase });
      showToast('Unlocked', 'success');
      await renderAccountSection(panel);
    } catch (e) {
      showToast('Unlock failed: ' + e, 'error');
    }
  });

  document.getElementById('sync-refresh-btn')?.addEventListener('click', async () => {
    try {
      await invoke('sync_refresh');
      showToast('Refreshed', 'success');
      await renderAccountSection(panel);
    } catch (e) {
      showToast('Refresh failed: ' + e, 'error');
    }
  });

  // Suppress unused warning
  void status;
}

function renderPrivacySection(panel) {
  panel.innerHTML = `
    <h2 class="settings-content-title">Privacy &amp; data</h2>

    <div class="settings-info-card">
      <h3 style="margin: 0 0 var(--space-2) 0;">Audio</h3>
      <p>
        <strong>Audio is never persisted, anywhere</strong> &mdash; not on this
        device, not in the cloud, not ever. While you record, audio lives only
        in a small ring buffer in RAM that feeds the streaming transcription
        provider over a TLS WebSocket. The buffer is overwritten continuously
        and dropped the moment you stop recording.
      </p>
      <p style="margin-top: var(--space-2); color: var(--color-text-secondary); font-size: var(--text-xs);">
        Verified at the schema layer in <code>src-tauri/src/store/db.rs</code>:
        the local database has no <code>audio_bytes</code>, <code>audio_blob</code>,
        or attachment column. There is no place to put audio even if a future
        change tried.
      </p>
    </div>

    <div class="settings-info-card" style="margin-top: var(--space-3);">
      <h3 style="margin: 0 0 var(--space-2) 0;">Transcripts &amp; notes</h3>
      <p>
        Only text transcripts and notes you write are saved, and only on this
        device. The local database is encrypted at rest using <strong>SQLCipher</strong>
        with a key stored in your operating system's secure credential vault
        &mdash; not on disk in plaintext, and never returned to the app's
        WebView.
      </p>
    </div>

    <div class="settings-info-card" style="margin-top: var(--space-3);">
      <h3 style="margin: 0 0 var(--space-2) 0;">Cloud providers</h3>
      <p>
        Real-time transcription (Deepgram, AssemblyAI) and LLM features
        (OpenAI, Anthropic, Gemini, Ollama) use <strong>your own API keys</strong>.
        Audire reads keys in Rust core only, fetched from your OS keyring or
        environment, and never exposes them to the WebView. All outbound
        traffic uses TLS.
      </p>
    </div>

    <div class="settings-info-card" style="margin-top: var(--space-3);">
      <h3 style="margin: 0 0 var(--space-2) 0;">Telemetry</h3>
      <p>
        <strong>None.</strong> Audire makes outbound network calls only to the
        providers you have configured. There is no analytics, no crash
        reporter, no usage tracking, no first-run beacon.
      </p>
    </div>

    <div class="settings-info-card" style="margin-top: var(--space-3);">
      <h3 style="margin: 0 0 var(--space-2) 0;">Audire Sync (optional)</h3>
      <p>
        Audire Sync is a separate, opt-in service. If you choose to sign in,
        transcripts and notes are end-to-end encrypted on this device before
        upload &mdash; the sync server stores ciphertext only and cannot read
        your data. <strong>Audio is still never synced</strong>, because it
        was never saved in the first place.
      </p>
      <p style="margin-top: var(--space-2); color: var(--color-text-secondary); font-size: var(--text-xs);">
        You are not signed in. Sync is off.
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
        btn.disabled = true;
        btn.textContent = 'Saving...';
        await invoke('save_api_key', { provider, key });
        input.value = '';
        showToast(`${provider} key saved`, 'success');
        await renderConnectorsSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to save key: ' + e, 'error');
      } finally {
        btn.disabled = false;
        btn.textContent = 'Save';
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
      if (!confirm(`Delete the saved ${provider} API key?`)) return;
      try {
        btn.disabled = true;
        await invoke('delete_api_key', { provider });
        showToast(`${provider} key deleted`, 'success');
        await renderConnectorsSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to delete key: ' + e, 'error');
      } finally {
        btn.disabled = false;
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
        btn.disabled = true;
        btn.textContent = 'Saving...';
        await invoke('save_calendar_config', { provider, clientId, clientSecret, tenantId });
        delete calendarErrors[provider];
        showToast(`${provider} calendar config saved`, 'success');
        await renderCalendarSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to save calendar config: ' + e, 'error');
      } finally {
        btn.disabled = false;
        btn.textContent = 'Save config';
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
      if (!confirm(`Disconnect ${provider} calendar? Upcoming events will stop appearing until you reconnect.`)) return;
      try {
        btn.disabled = true;
        await invoke('disconnect_calendar_provider', { provider });
        delete calendarErrors[provider];
        showToast(`${provider} calendar disconnected`, 'success');
        await renderCalendarSection(document.getElementById('settings-content-panel'));
      } catch (e) {
        showToast('Failed to disconnect: ' + e, 'error');
      } finally {
        btn.disabled = false;
      }
    });
  });
}
