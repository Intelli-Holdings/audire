// Home view — "Coming up" calendar events + past meeting notes + bottom ask bar

import { invoke } from '@tauri-apps/api/core';
import { showToast } from '../toast.js';
import { showView, startCapture } from '../sidebar.js';
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
  const weekday = date.toLocaleDateString('en-US', { weekday: 'short' });
  const day = date.getDate();
  const month = date.toLocaleDateString('en-US', { month: 'short' });
  return `${weekday} ${day} ${month}`;
}

function formatClockTime(date) {
  if (isNaN(date.getTime())) return '';
  return date.toLocaleTimeString('en-US', {
    hour: '2-digit', minute: '2-digit', hour12: false,
  });
}

/** Parse ISO 8601 or bare YYYY-MM-DD date strings safely.
 *  Bare date strings are treated as local midnight (not UTC). */
function parseEventDate(str) {
  if (!str) return new Date(NaN);
  return /^\d{4}-\d{2}-\d{2}$/.test(str)
    ? new Date(str + 'T00:00:00')
    : new Date(str);
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

/** Group calendar events by date key (YYYY-MM-DD) */
function groupEventsByDate(events) {
  const groups = new Map();
  for (const ev of events) {
    const start = parseEventDate(ev.start);
    const key = `${start.getFullYear()}-${String(start.getMonth() + 1).padStart(2, '0')}-${String(start.getDate()).padStart(2, '0')}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(ev);
  }
  return groups;
}

function todayKey() {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
}

export function initHomeView(state, callbacks = {}) {
  appState = state;
  onNavigateToTranscript = callbacks.onNavigateToTranscript || null;
}

/* ── Skeleton loader ─────────────────────────────────────────── */

function renderSkeleton() {
  const skeletonDateGroup = (w) => `
    <div class="calendar-date-group">
      <div class="calendar-event-date">
        <div class="skeleton-block" style="width:30px;height:28px;border-radius:4px;"></div>
        <div class="skeleton-block" style="width:34px;height:9px;margin-top:4px;border-radius:3px;"></div>
        <div class="skeleton-block" style="width:22px;height:9px;margin-top:2px;border-radius:3px;"></div>
      </div>
      <div class="calendar-date-events">
        <div class="calendar-event-entry">
          <div class="skeleton-bar"></div>
          <div class="calendar-event-info">
            <div class="skeleton-block" style="width:${w}px;height:13px;border-radius:3px;"></div>
            <div class="skeleton-block" style="width:80px;height:10px;margin-top:4px;border-radius:3px;"></div>
          </div>
        </div>
      </div>
    </div>`;

  const skeletonMeeting = (tw, sw) => `
    <div class="meeting-list-item" style="pointer-events:none;">
      <div class="skeleton-block" style="width:32px;height:32px;border-radius:50%;flex-shrink:0;"></div>
      <div class="meeting-list-info">
        <div class="skeleton-block" style="width:${tw}px;height:13px;border-radius:3px;"></div>
        <div class="skeleton-block" style="width:${sw}px;height:10px;margin-top:4px;border-radius:3px;"></div>
      </div>
      <div class="skeleton-block" style="width:36px;height:11px;border-radius:3px;flex-shrink:0;"></div>
    </div>`;

  return `
    <div class="home-view">
      <h2 class="home-section-title">Coming up</h2>
      <div class="calendar-events-card skeleton-pulse">
        ${skeletonDateGroup(180)}
        ${skeletonDateGroup(210)}
        ${skeletonDateGroup(160)}
        ${skeletonDateGroup(240)}
      </div>
      <div class="skeleton-pulse">
        <div class="skeleton-block" style="width:80px;height:10px;border-radius:3px;margin-bottom:var(--space-3);"></div>
        ${skeletonMeeting(200, 100)}
        ${skeletonMeeting(260, 80)}
      </div>
      <div class="skeleton-pulse" style="margin-top:var(--space-4);">
        <div class="skeleton-block" style="width:70px;height:10px;border-radius:3px;margin-bottom:var(--space-3);"></div>
        ${skeletonMeeting(280, 130)}
        ${skeletonMeeting(170, 110)}
      </div>
      <div class="home-composer" style="opacity:0.4;">
        <textarea class="ask-input prompt-textarea" placeholder="Ask anything" rows="1" disabled></textarea>
        <button class="recipe-shortcut-btn" disabled>/ List recent todos</button>
      </div>
    </div>`;
}

/* ── Main render ─────────────────────────────────────────────── */

export async function renderHomeView() {
  const container = document.getElementById('view-home');
  if (!container) return;

  // Show skeleton immediately
  container.innerHTML = renderSkeleton();

  // Load data in parallel
  let events = [];
  let meetings = [];
  let calendarStatuses = [];
  try {
    [events, meetings, calendarStatuses] = await Promise.all([
      invoke('list_upcoming_calendar_events').catch(() => []),
      invoke('list_meetings').catch((e) => {
        console.error('list_meetings error:', e);
        return [];
      }),
      invoke('list_calendar_statuses').catch(() => []),
    ]);
  } catch { /* fallback */ }

  // ── Build connected-accounts status line ────────────────────
  const connectedAccounts = calendarStatuses.filter(s => s.connected && s.email);
  let calendarStatusHtml = '';
  if (connectedAccounts.length > 0) {
    const labels = connectedAccounts.map(a =>
      `<span class="home-cal-account">${escapeHtml(a.email)}</span>`
    ).join('');
    calendarStatusHtml = `<div class="home-calendar-status">Connected ${labels}</div>`;
  }

  // ── Build calendar events grouped by date ──────────────────
  let eventsHtml = '';
  if (events.length > 0) {
    const grouped = groupEventsByDate(events);
    const tk = todayKey();

    // Ensure today is shown first
    const displayMap = grouped.has(tk) ? grouped : (() => {
      const m = new Map([[tk, []]]);
      for (const [k, v] of grouped) m.set(k, v);
      return m;
    })();

    let groupRows = '';
    for (const [dateKey, dateEvents] of displayMap) {
      const sampleDate = dateEvents.length > 0
        ? parseEventDate(dateEvents[0].start)
        : new Date(dateKey + 'T00:00:00');
      const dateNum = isNaN(sampleDate.getTime()) ? '--' : sampleDate.getDate();
      const monthStr = sampleDate.toLocaleDateString('en-US', { month: 'long' });
      const dayStr = sampleDate.toLocaleDateString('en-US', { weekday: 'short' });
      const isToday = dateKey === tk;

      let entriesHtml = '';
      if (dateEvents.length === 0) {
        entriesHtml = `
          <div class="calendar-event-entry">
            <div class="calendar-event-info">
              <span class="calendar-event-title calendar-event-muted">No more events today</span>
            </div>
          </div>`;
      } else {
        for (const ev of dateEvents) {
          const start = parseEventDate(ev.start);
          const end = parseEventDate(ev.end);
          const timeRange = `${formatClockTime(start)} \u2013 ${formatClockTime(end)}`;
          const attendeeCount = ev.attendees?.length || 0;
          const attendeeHint = attendeeCount > 0
            ? `<span class="calendar-event-attendees">${attendeeCount} attendee${attendeeCount > 1 ? 's' : ''}</span>`
            : '';
          entriesHtml += `
            <div class="calendar-event-entry" data-event-id="${escapeHtml(ev.external_id)}">
              <div class="calendar-event-bar"></div>
              <div class="calendar-event-info">
                <span class="calendar-event-title">${escapeHtml(ev.title || 'Untitled event')}</span>
                <span class="calendar-event-time">${escapeHtml(timeRange)}${attendeeHint}</span>
              </div>
              <button class="calendar-event-record-btn" type="button" title="Record this meeting" aria-label="Record ${escapeHtml(ev.title || 'this meeting')}">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="12" r="8"/></svg>
              </button>
            </div>`;
        }
      }

      groupRows += `
        <div class="calendar-date-group${isToday ? ' calendar-date-today' : ''}">
          <div class="calendar-event-date">
            <span class="calendar-event-date-num">${dateNum}</span>
            <span class="calendar-event-date-month">${escapeHtml(monthStr)}${isToday ? '<span class="today-dot"></span>' : ''}</span>
            <span class="calendar-event-date-day">${escapeHtml(dayStr)}</span>
          </div>
          <div class="calendar-date-events">
            ${entriesHtml}
          </div>
        </div>`;
    }
    eventsHtml = `<div class="calendar-events-card">${groupRows}</div>`;
  } else {
    eventsHtml = `
      <div class="calendar-events-card">
        <div class="calendar-empty">No upcoming events. Connect your calendar in Settings.</div>
      </div>`;
  }

  // ── Build meeting notes list grouped by date ───────────────
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
      const subtitle = m.note_preview
        ? escapeHtml(m.note_preview.slice(0, 60))
        : 'Me';
      meetingListHtml += `
        <button class="meeting-list-item" data-mid="${escapeHtml(m.id)}">
          <div class="meeting-list-avatar">${escapeHtml(initial)}</div>
          <div class="meeting-list-info">
            <div class="meeting-list-title truncate">${escapeHtml(title)}</div>
            <div class="meeting-list-meta">${subtitle}</div>
          </div>
          <div class="meeting-list-right">
            <svg class="meeting-list-lock" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect width="18" height="11" x="3" y="11" rx="2" ry="2"/>
              <path d="M7 11V7a5 5 0 0 1 10 0v4"/>
            </svg>
            <span class="meeting-list-time">${escapeHtml(timeStr)}</span>
          </div>
        </button>`;
    }
  }

  if (!meetingListHtml) {
    meetingListHtml = `
      <div style="padding: var(--space-8) 0; text-align: center;">
        <p class="text-muted text-sm">No meeting notes yet. Start a recording session to create your first note.</p>
      </div>`;
  }

  // ── Render final HTML ──────────────────────────────────────
  container.innerHTML = `
    <div class="home-view">
      <h2 class="home-section-title">Coming up</h2>
      ${calendarStatusHtml}
      ${eventsHtml}
      ${meetingListHtml}
      <div class="home-composer">
        <textarea class="ask-input prompt-textarea" id="home-ask-input" placeholder="Ask anything" rows="1"></textarea>
        <button class="recipe-shortcut-btn" id="home-recipe-btn">/ List recent todos</button>
      </div>
    </div>`;

  // ── Bind events ────────────────────────────────────────────
  container.querySelectorAll('[data-mid]').forEach(el => {
    el.addEventListener('click', () => {
      appState.selectedMeetingNoteId = el.dataset.mid;
      appState.selectedStandaloneNoteId = null;
      appState.selectedFolderId = null;
      showView('notes');
    });
  });

  // Calendar event record buttons — start capture with attendees auto-imported
  container.querySelectorAll('.calendar-event-record-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const entry = btn.closest('[data-event-id]');
      if (!entry) return;
      const eventId = entry.dataset.eventId;
      const ev = events.find(ev => ev.external_id === eventId);
      if (ev) {
        startCapture(ev);
      }
    });
  });

  // Ask input
  const askInput = document.getElementById('home-ask-input');
  setupAutosizeTextarea(askInput, { minRows: 1, maxVh: 0.28 });
  bindTextareaSubmit(askInput, async () => {
    const query = askInput.value.trim();
    if (!query) return;
    const recipeBtn = document.getElementById('home-recipe-btn');
    const originalPlaceholder = askInput.placeholder;
    setTextareaValue(askInput, '', { scrollToEnd: true });
    askInput.disabled = true;
    askInput.placeholder = 'Thinking...';
    if (recipeBtn) recipeBtn.disabled = true;
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
    } finally {
      askInput.disabled = false;
      askInput.placeholder = originalPlaceholder;
      if (recipeBtn) recipeBtn.disabled = false;
      askInput.focus();
    }
  });

  // Recipe shortcut — uses the canonical recipe ID from src-tauri/src/llm/recipe.rs.
  document.getElementById('home-recipe-btn')?.addEventListener('click', () => {
    setTextareaValue(askInput, '/recent_todos', { focus: true, scrollToEnd: true });
  });
}
