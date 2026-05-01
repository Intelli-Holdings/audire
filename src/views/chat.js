// Chat view — Granola-style dedicated chat with greeting, composer, recents, recipes

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';

let appState = null;
let chatHistory = [];
let recognition = null;
let isListening = false;
// Scope is passed verbatim to ask_audire / ask_audire_llm. Backend recognises
// "all" (global), "meeting" (with meeting_id), and "folder" (with folder_id).
let currentScope = { id: 'all', label: 'All meetings', meetingId: null };

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

function meetingDisplayName(m) {
  if (m.title && m.title !== 'Untitled' && m.title.trim()) return m.title;
  // Fallback: use date + time
  if (m.started_at) {
    const d = new Date(m.started_at * 1000);
    return 'Meeting ' + d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
      + ', ' + d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
  }
  return 'Meeting';
}

function formatRelativeShort(tsMs) {
  if (!tsMs) return '';
  const diff = Date.now() - tsMs;
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d`;
  const weeks = Math.floor(days / 7);
  if (weeks < 5) return `${weeks}w`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo`;
  return `${Math.floor(days / 365)}y`;
}

function getChatGreetingName() {
  try {
    const stored = localStorage.getItem('audire.user.displayName');
    if (stored && stored.trim()) return stored.trim().split(/\s+/)[0];
  } catch { /* ignore */ }
  const raw = document.querySelector('.user-card-name')?.textContent?.trim();
  if (raw && raw !== 'Audire User' && raw !== 'User') {
    return raw.split(/\s+/)[0];
  }
  return 'there';
}

export function initChatView(state) {
  appState = state;
}

export async function renderChatView() {
  const container = document.getElementById('view-chat');
  if (!container) return;

  // Pull recent meetings as "Recents" in the chat view.
  let recentMeetings = [];
  let llmProviders = [];
  let detectionSettings = {};
  try {
    [recentMeetings, llmProviders, detectionSettings] = await Promise.all([
      invoke('list_meetings').catch(() => []),
      invoke('list_llm_providers').catch(() => []),
      invoke('get_detection_settings').catch(() => ({})),
    ]);
  } catch { /* ignore */ }
  const recentsItems = [...(recentMeetings || [])]
    .sort((a, b) => (b.started_at || 0) - (a.started_at || 0))
    .slice(0, 3);

  const greetingName = getChatGreetingName();

  // Determine the active LLM provider label. Prefer the user's preferred
  // provider when it's available; otherwise fall back to the first available
  // provider; otherwise show a "Set up AI" prompt that links to settings.
  const preferredId = detectionSettings.preferred_llm_provider || '';
  const availableProviders = llmProviders.filter((p) => p.available);
  const activeProvider =
    availableProviders.find((p) => p.id === preferredId) ||
    availableProviders[0] ||
    null;
  const modelLabel = activeProvider ? activeProvider.name : 'Set up AI';

  const recentsHtml = recentsItems.length
    ? recentsItems.map(m => `
        <a class="chat-recent-item" href="#" data-meeting-id="${escapeHtml(m.id || '')}">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
          <span class="chat-recent-title">${escapeHtml(meetingDisplayName(m))}</span>
          <span class="chat-recent-ago">${escapeHtml(formatRelativeShort((m.started_at || 0) * 1000))}</span>
        </a>
      `).join('')
    : '';

  container.innerHTML = `
    <div class="chat-view">
      <div class="chat-view-inner">
        <h1 class="chat-greeting">Hi ${escapeHtml(greetingName)}, ask anything</h1>

        <div class="chat-composer">
          <div class="chat-composer-top">
            <div class="chat-scope-row">
              <button class="scope-dropdown" id="chat-scope-btn" type="button" aria-haspopup="menu" aria-expanded="false">
                <span class="scope-dropdown-primary" id="chat-scope-primary">Search</span>
                <span class="scope-dropdown-secondary" id="chat-scope-secondary">${escapeHtml(currentScope.label)}</span>
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>
              </button>
              <div class="scope-popover" id="chat-scope-popover" role="menu" hidden></div>
            </div>
            <textarea class="chat-textarea" id="chat-view-input" placeholder="Summarize my meetings this week" rows="1"></textarea>
          </div>
          <div class="chat-composer-bottom">
            <div class="chat-composer-meta">
              <button class="chat-model-pill" type="button" id="chat-model-pill" title="Manage AI providers">
                <span class="chat-model-label">${escapeHtml(modelLabel)}</span>
                <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>
              </button>
            </div>
            <button class="chat-send-icon" id="chat-send-btn" type="button" aria-label="Start voice input">
              <svg class="chat-send-arrow" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 19V5M5 12l7-7 7 7"/></svg>
              <svg class="chat-send-mic" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" y1="19" x2="12" y2="22"/></svg>
            </button>
          </div>
        </div>

        <div id="chat-view-messages" class="chat-messages-list"></div>

        ${recentsHtml ? `
        <div class="chat-section">
          <h3 class="chat-section-label">Recents</h3>
          <div class="chat-recents">${recentsHtml}</div>
        </div>
        ` : ''}

        <div class="chat-section">
          <h3 class="chat-section-label">Recipes</h3>
          <div class="chat-recipes">
            <button class="recipe-chip" data-recipe="List recent todos"><span class="chip-icon recipe-green">/</span> List recent todos</button>
            <button class="recipe-chip" data-recipe="Coach me"><span class="chip-icon recipe-amber">/</span> Coach me</button>
            <button class="recipe-chip" data-recipe="Write weekly recap"><span class="chip-icon recipe-amber">/</span> Write weekly recap</button>
            <button class="recipe-chip" data-recipe="Streamline my calendar"><span class="chip-icon recipe-blue">/</span> Streamline my calendar</button>
            <button class="recipe-chip" data-recipe="Blind spots"><span class="chip-icon recipe-blue">/</span> Blind spots</button>
          </div>
        </div>
      </div>
    </div>
  `;

  const chatInput = document.getElementById('chat-view-input');
  const sendBtn = document.getElementById('chat-send-btn');

  // Toggle mic vs send icon based on whether there's text to send.
  const syncSendIcon = () => {
    if (!sendBtn) return;
    const hasText = Boolean(chatInput?.value.trim());
    sendBtn.classList.toggle('has-text', hasText);
    sendBtn.setAttribute('aria-label', hasText ? 'Send' : 'Start voice input');
  };
  chatInput?.addEventListener('input', syncSendIcon);
  syncSendIcon();

  // Scope dropdown — opens a popover listing scopes the backend supports.
  setupScopePicker();

  // Model pill jumps to AI Provider settings so the user can pick / configure.
  document.getElementById('chat-model-pill')?.addEventListener('click', () => {
    document.dispatchEvent(new CustomEvent('audire:navigate', { detail: { view: 'settings' } }));
  });

  // Recipe chip -> prefill composer
  container.querySelectorAll('.recipe-chip[data-recipe]').forEach(chip => {
    chip.addEventListener('click', () => {
      if (!chatInput) return;
      chatInput.value = chip.dataset.recipe;
      chatInput.focus();
      syncSendIcon();
    });
  });

  // Clicking a recent meeting jumps to the transcript view.
  container.querySelectorAll('.chat-recent-item[data-meeting-id]').forEach(el => {
    el.addEventListener('click', (e) => {
      e.preventDefault();
      const id = el.dataset.meetingId;
      if (!id || !appState) return;
      appState.meetingId = id;
      // Best-effort: request transcript view (main.js handles the switch if listener registered).
      document.dispatchEvent(new CustomEvent('audire:navigate', { detail: { view: 'transcript' } }));
    });
  });

  async function sendQuery() {
    const query = chatInput?.value.trim();
    if (!query) return;
    chatInput.value = '';
    syncSendIcon();

    appendMessage('user', query);
    chatHistory.push({ query, answer: '' });

    try {
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
        scope: currentScope.id,
        meetingId: currentScope.meetingId,
        folderId: null,
      });
      const answer = resp.answer || 'No response.';
      const citations = resp.citations || [];
      appendMessage('audire', answer, citations);
      chatHistory[chatHistory.length - 1].answer = answer;
    } catch (err) {
      appendMessage('audire', 'Error: ' + err);
    }
  }

  chatInput?.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      await sendQuery();
    }
  });

  sendBtn?.addEventListener('click', async () => {
    if (sendBtn.classList.contains('has-text')) {
      await sendQuery();
    } else {
      await toggleVoiceInput(chatInput, sendBtn, syncSendIcon);
    }
  });
}

async function toggleVoiceInput(chatInput, sendBtn, syncSendIcon) {
  if (isListening && recognition) {
    recognition.stop();
    return;
  }

  const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!SpeechRecognition) {
    showToast('Speech recognition is not supported in this build. Voice input is unavailable on this platform.', 'error');
    return;
  }

  // Probe mic permission via getUserMedia first. This is what actually triggers
  // the OS permission prompt on macOS / WebView2; without it the WebView's
  // SpeechRecognition request is rejected silently as "not-allowed".
  // We release the stream immediately — SpeechRecognition opens its own.
  if (navigator?.mediaDevices?.getUserMedia) {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((t) => t.stop());
    } catch (err) {
      const reason = err?.name || 'unknown';
      if (reason === 'NotAllowedError' || reason === 'SecurityError') {
        showToast('Microphone access was denied. Grant mic permission to Audire in your OS settings, then try again.', 'error');
      } else if (reason === 'NotFoundError' || reason === 'OverconstrainedError') {
        showToast('No microphone was found on this device.', 'error');
      } else {
        showToast('Mic unavailable: ' + reason, 'error');
      }
      return;
    }
  }

  recognition = new SpeechRecognition();
  recognition.continuous = true;
  recognition.interimResults = true;
  recognition.lang = 'en-US';

  // Track what was in the textarea before we started, and accumulate finals.
  const baseline = chatInput?.value || '';
  let accumulated = '';

  recognition.onstart = () => {
    isListening = true;
    sendBtn?.classList.add('is-listening');
    sendBtn?.setAttribute('aria-label', 'Stop voice input');
  };

  recognition.onresult = (event) => {
    let interim = '';
    accumulated = '';
    for (let i = 0; i < event.results.length; i++) {
      const transcript = event.results[i][0].transcript;
      if (event.results[i].isFinal) {
        accumulated += transcript;
      } else {
        interim += transcript;
      }
    }
    if (chatInput) {
      chatInput.value = baseline + accumulated + interim;
      syncSendIcon();
    }
  };

  recognition.onerror = (event) => {
    if (event.error === 'aborted' || event.error === 'no-speech') {
      // Aborted: user pressed stop. no-speech: silence timeout. Both are fine.
    } else if (event.error === 'not-allowed' || event.error === 'service-not-allowed') {
      showToast('Microphone access was denied. Grant mic permission to Audire in your OS settings, then try again.', 'error');
    } else if (event.error === 'audio-capture') {
      showToast('No microphone was found on this device.', 'error');
    } else if (event.error === 'network') {
      showToast('Speech recognition needs an internet connection. Check your network and try again.', 'error');
    } else {
      showToast('Mic error: ' + event.error, 'error');
    }
    stopListening(sendBtn);
  };

  recognition.onend = () => {
    stopListening(sendBtn);
    syncSendIcon();
    chatInput?.focus();
  };

  recognition.start();
}

function stopListening(sendBtn) {
  isListening = false;
  recognition = null;
  sendBtn?.classList.remove('is-listening');
  sendBtn?.setAttribute('aria-label', 'Start voice input');
}

function setupScopePicker() {
  const btn = document.getElementById('chat-scope-btn');
  const popover = document.getElementById('chat-scope-popover');
  const secondary = document.getElementById('chat-scope-secondary');
  if (!btn || !popover || !secondary) return;

  // Available scopes mirror what the Rust retrieval layer accepts.
  // "Current meeting" is only offered when the user opened the chat from a
  // specific meeting (appState.meetingId set by transcript view).
  const options = [
    { id: 'all', label: 'All meetings', meetingId: null },
  ];
  if (appState?.meetingId) {
    options.push({ id: 'meeting', label: 'Current meeting only', meetingId: appState.meetingId });
  }

  popover.innerHTML = options.map((opt, i) => `
    <button class="scope-popover-item" type="button" role="menuitem" data-scope-idx="${i}">
      ${escapeHtml(opt.label)}
    </button>
  `).join('');

  const close = () => {
    popover.hidden = true;
    btn.setAttribute('aria-expanded', 'false');
  };
  const open = () => {
    popover.hidden = false;
    btn.setAttribute('aria-expanded', 'true');
  };

  btn.addEventListener('click', (e) => {
    e.stopPropagation();
    if (popover.hidden) open(); else close();
  });

  popover.addEventListener('click', (e) => {
    const item = e.target.closest('.scope-popover-item');
    if (!item) return;
    const idx = Number(item.dataset.scopeIdx);
    const opt = options[idx];
    if (!opt) return;
    currentScope = opt;
    secondary.textContent = opt.label;
    close();
  });

  // Click outside closes the popover.
  document.addEventListener('click', (e) => {
    if (popover.hidden) return;
    if (popover.contains(e.target) || btn.contains(e.target)) return;
    close();
  });
}

function appendMessage(role, text, citations = []) {
  const container = document.getElementById('chat-view-messages');
  if (!container) return;
  const msg = document.createElement('div');
  msg.className = 'chat-message';
  msg.dataset.role = role;

  let citationsHtml = '';
  if (citations.length > 0) {
    citationsHtml = `<div class="chat-citations">${citations.slice(0, 3).map(c =>
      `<span class="badge-subtle text-xs">${escapeHtml(c.title || c.excerpt?.slice(0, 40) || 'source')}</span>`
    ).join('')}</div>`;
  }

  let actionsHtml = '';
  if (role === 'audire') {
    const msgId = Date.now();
    actionsHtml = `
      <div class="chat-message-actions">
        <button class="btn-ghost btn-xs chat-copy-btn" data-copy-text="${escapeHtml(text)}">Copy</button>
        <button class="btn-ghost btn-xs chat-save-btn" data-save-text="${escapeHtml(text)}">Save as note</button>
      </div>
    `;
  }

  msg.innerHTML = `
    <span class="chat-message-role">${role === 'user' ? 'You' : 'Audire'}</span>
    <span class="chat-message-body">${escapeHtml(text)}</span>
    ${citationsHtml}
    ${actionsHtml}
  `;

  // Bind action buttons
  msg.querySelector('.chat-copy-btn')?.addEventListener('click', async (e) => {
    try {
      await navigator.clipboard.writeText(text);
      e.target.textContent = 'Copied!';
      setTimeout(() => { e.target.textContent = 'Copy'; }, 1500);
    } catch { showToast('Copy failed', 'error'); }
  });

  msg.querySelector('.chat-save-btn')?.addEventListener('click', async (e) => {
    try {
      await invoke('create_standalone_note', { title: 'Chat insight', text });
      e.target.textContent = 'Saved!';
      setTimeout(() => { e.target.textContent = 'Save as note'; }, 1500);
    } catch { showToast('Save failed', 'error'); }
  });

  container.appendChild(msg);
  container.scrollTop = container.scrollHeight;
}
