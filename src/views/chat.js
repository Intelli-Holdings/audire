// Chat view — dedicated chat with greeting, input, recents, recipes

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';

let appState = null;
let chatHistory = [];

function escapeHtml(str) {
  const d = document.createElement('div');
  d.textContent = str ?? '';
  return d.innerHTML;
}

export function initChatView(state) {
  appState = state;
}

export async function renderChatView() {
  const container = document.getElementById('view-chat');
  if (!container) return;

  const recentsHtml = chatHistory.length > 0
    ? chatHistory.slice(-5).reverse().map(item => `
        <div class="chat-recent-item">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
          <span class="truncate">${escapeHtml(item.query.slice(0, 80))}</span>
        </div>
      `).join('')
    : `<p class="text-muted text-sm" style="padding: var(--space-2) var(--space-3);">No recent chats.</p>`;

  container.innerHTML = `
    <div class="chat-view">
      <h1 class="chat-greeting">Hi, ask anything</h1>

      <div class="chat-input-area">
        <div class="chat-scope-row">
          <select class="chat-scope-select" id="chat-scope-select">
            <option value="all">All notes</option>
            <option value="mynotes">My notes</option>
          </select>
          <span class="chat-model-label">Audire AI</span>
        </div>
        <input type="text" class="ask-input" id="chat-view-input" placeholder="Ask anything..." />
      </div>

      <div id="chat-view-messages" style="display:flex;flex-direction:column;gap:var(--space-3);margin-bottom:var(--space-6);"></div>

      <div>
        <div class="chat-recents-title">Recent</div>
        <div class="chat-recents-list" id="chat-recents-list">
          ${recentsHtml}
        </div>
      </div>

      <div>
        <div class="chat-recipes-title">Recipes</div>
        <div class="recipe-pills">
          <button class="recipe-pill" data-recipe="summary">Summarize recent meetings</button>
          <button class="recipe-pill" data-recipe="todos">List recent todos</button>
          <button class="recipe-pill" data-recipe="weekly_recap">Write weekly recap</button>
          <button class="recipe-pill" data-recipe="coach">Coach me</button>
        </div>
      </div>
    </div>
  `;

  // Bind events
  const chatInput = document.getElementById('chat-view-input');

  chatInput?.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      const query = chatInput.value.trim();
      if (!query) return;
      chatInput.value = '';
      const scope = document.getElementById('chat-scope-select')?.value || 'all';

      appendMessage('user', query);
      chatHistory.push({ query, answer: '' });

      try {
        let hasLlm = false;
        try {
          const hasAnthropic = await invoke('has_api_key', { provider: 'anthropic' });
          const hasOpenai = await invoke('has_api_key', { provider: 'openai' });
          hasLlm = hasAnthropic || hasOpenai;
        } catch { /* ignore */ }

        const command = hasLlm ? 'ask_audire_llm' : 'ask_audire';
        const resp = await invoke(command, {
          query,
          scope,
          meetingId: null,
          folderId: null,
        });
        const answer = resp.answer || 'No response.';
        appendMessage('audire', answer);
        chatHistory[chatHistory.length - 1].answer = answer;
      } catch (err) {
        appendMessage('audire', 'Error: ' + err);
      }
    }
  });

  // Recipe pills
  container.querySelectorAll('[data-recipe]').forEach(btn => {
    btn.addEventListener('click', () => {
      chatInput.value = '/' + btn.dataset.recipe;
      chatInput.focus();
    });
  });
}

function appendMessage(role, text) {
  const container = document.getElementById('chat-view-messages');
  if (!container) return;
  const msg = document.createElement('div');
  msg.className = 'chat-message';
  msg.dataset.role = role;
  msg.innerHTML = `
    <span class="chat-message-role">${role === 'user' ? 'You' : 'Audire'}</span>
    <span class="chat-message-body">${escapeHtml(text)}</span>
  `;
  container.appendChild(msg);
  container.scrollTop = container.scrollHeight;
}
