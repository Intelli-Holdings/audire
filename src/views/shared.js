// Shared with me view — empty state (no backend sharing system yet)

let appState = null;

export function initSharedView(state) {
  appState = state;
}

export async function renderSharedView() {
  const container = document.getElementById('view-shared');
  if (!container) return;

  container.innerHTML = `
    <div class="shared-view">
      <h2 class="shared-title">Shared with me</h2>
      <p class="shared-subtitle">Notes and folders shared with you by others will appear here.</p>

      <div class="empty-state">
        <div class="empty-state-icon">
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/>
            <circle cx="9" cy="7" r="4"/>
            <path d="M22 21v-2a4 4 0 0 0-3-3.87"/>
            <path d="M16 3.13a4 4 0 0 1 0 7.75"/>
          </svg>
        </div>
        <p class="empty-state-title">No shared notes yet</p>
        <p class="empty-state-body">When someone shares a note or folder with you, it will show up here.</p>
      </div>
    </div>
  `;
}
