// Home view — "Coming up" calendar events + past meeting notes + bottom ask bar

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';

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

function groupMeetingsByDate(meetings) {
  const groups = {};
  for (const m of meetings) {
    const key = formatRelativeDate(m.started_at);
    if (!groups[key]) groups[key] = [];
    groups[key].push(m);
  }
  return groups;
}

export function initHomeView(state, callbacks = {}) {
  appState = state;
  onNavigateToTranscript = callbacks.onNavigateToTranscript || null;
}

export async function renderHomeView() {
  const container = document.getElementById('view-home');
  if (!container) return;

  // Load data
  let events = [];
  let meetings = [];
  try {
    events = await invoke('list_calendar_events');
  } catch { /* calendar may not be connected */ }
  try {
    meetings = await invoke('list_meetings');
  } catch (e) {
    console.error('list_meetings error:', e);
  }

  // Build calendar events
  let eventsHtml = '';
  if (events.length > 0) {
    const eventRows = events.slice(0, 5).map(ev => {
      const start = ev.start ? new Date(ev.start * 1000) : null;
      const end = ev.end ? new Date(ev.end * 1000) : null;
      const dateNum = start ? start.getDate() : '';
      const monthStr = start ? start.toLocaleDateString('en-US', { month: 'short' }).toUpperCase() : '';
      const dayStr = start ? start.toLocaleDateString('en-US', { weekday: 'short' }) : '';
      const timeRange = start && end
        ? `${formatClockTime(start)} – ${formatClockTime(end)}`
        : start ? formatClockTime(start) : '';

      return `
        <div class="calendar-event-row">
          <div class="calendar-event-date">
            <span class="calendar-event-date-num">${dateNum}</span>
            <span class="calendar-event-date-month">${escapeHtml(monthStr)}</span>
            <span class="calendar-event-date-day">${escapeHtml(dayStr)}</span>
          </div>
          <div class="calendar-event-bar"></div>
          <div class="calendar-event-info">
            <span class="calendar-event-title">${escapeHtml(ev.title || 'Untitled event')}</span>
            <span class="calendar-event-time">${escapeHtml(timeRange)}</span>
          </div>
        </div>
      `;
    }).join('');
    eventsHtml = `<div class="calendar-events-card">${eventRows}</div>`;
  } else {
    eventsHtml = `
      <div class="calendar-events-card">
        <div class="calendar-empty">No upcoming events. Connect your calendar in Settings.</div>
      </div>
    `;
  }

  // Build meeting notes list grouped by date
  const sorted = [...meetings]
    .filter(m => m.note_count > 0 || m.has_structured_notes)
    .sort((a, b) => (b.started_at || 0) - (a.started_at || 0));
  const groups = groupMeetingsByDate(sorted);

  let meetingListHtml = '';
  for (const [dateLabel, items] of Object.entries(groups)) {
    meetingListHtml += `<div class="meeting-list-date-header">${escapeHtml(dateLabel)}</div>`;
    for (const m of items) {
      const title = m.title || 'Meeting notes';
      const initial = (title.trim().charAt(0) || 'M').toUpperCase();
      const date = m.started_at ? new Date(m.started_at * 1000) : null;
      const timeStr = date ? formatClockTime(date) : '';
      meetingListHtml += `
        <button class="meeting-list-item" data-mid="${escapeHtml(m.id)}">
          <div class="meeting-list-avatar">${escapeHtml(initial)}</div>
          <div class="meeting-list-info">
            <div class="meeting-list-title truncate">${escapeHtml(title)}</div>
            <div class="meeting-list-meta">${m.note_count || 0} note${m.note_count === 1 ? '' : 's'}</div>
          </div>
          <div class="meeting-list-time">${escapeHtml(timeStr)}</div>
        </button>
      `;
    }
  }

  if (!meetingListHtml) {
    meetingListHtml = `
      <div style="padding: var(--space-8) 0; text-align: center;">
        <p class="text-muted text-sm">No meeting notes yet. Start a recording session to create your first note.</p>
      </div>
    `;
  }

  container.innerHTML = `
    <div class="home-view">
      <h2 class="home-section-title">Coming up</h2>
      ${eventsHtml}
      ${meetingListHtml}
    </div>
    <div class="view-bottom-bar">
      <input type="text" class="ask-input" id="home-ask-input" placeholder="Ask anything" />
      <button class="recipe-shortcut-btn" id="home-recipe-btn">/ List recent todos</button>
    </div>
  `;

  // Bind events
  container.querySelectorAll('[data-mid]').forEach(el => {
    el.addEventListener('click', () => {
      appState.meetingId = el.dataset.mid;
      if (onNavigateToTranscript) onNavigateToTranscript();
    });
  });

  // Ask input
  const askInput = document.getElementById('home-ask-input');
  askInput?.addEventListener('keydown', async (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      const query = askInput.value.trim();
      if (!query) return;
      askInput.value = '';
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
          scope: 'all',
          meetingId: null,
          folderId: null,
        });
        showToast(resp.answer?.slice(0, 100) || 'Done', 'success');
      } catch (err) {
        showToast('Error: ' + err, 'error');
      }
    }
  });

  // Recipe shortcut
  document.getElementById('home-recipe-btn')?.addEventListener('click', () => {
    askInput.value = '/todos';
    askInput.focus();
  });
}
