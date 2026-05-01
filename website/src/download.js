// Smart-download module.
//
// Detects the visitor's OS and surfaces the matching binary from the latest
// GitHub release. Falls back gracefully if the GitHub API is rate-limited or
// unreachable.
//
// GitHub's public API allows 60 unauthenticated requests per hour per IP.
// We cache the response in localStorage for an hour to stay well under that
// even on a busy day.

const REPO = 'Intelli-Holdings/audire';
const RELEASES_API = `https://api.github.com/repos/${REPO}/releases/latest`;
const RELEASES_PAGE = `https://github.com/${REPO}/releases/latest`;
const CACHE_KEY = 'audire.gh.releases.latest.v1';
const CACHE_TTL_MS = 60 * 60 * 1000; // 1 hour

const OS_LABELS = {
  windows: 'Windows',
  macos: 'macOS',
  linux: 'Linux',
  unknown: 'your platform',
};

const OS_ICONS = {
  windows: `
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M3 5.2 10.9 4v7.2H3V5.2Zm9.1-1.4L21 2.5v8.7h-8.9V3.8ZM3 12.8h7.9V20L3 18.8v-6Zm9.1 0H21v8.7l-8.9-1.3v-7.4Z"/>
    </svg>
  `,
  macos: `
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M16.4 2.2c.1 1.4-.4 2.7-1.3 3.7-.9 1-2.1 1.7-3.4 1.6-.1-1.3.4-2.6 1.3-3.6.9-1 2.3-1.7 3.4-1.7Zm4 15.2c-.5 1.1-.8 1.6-1.5 2.6-1 1.4-2.4 3.1-4.1 3.1-1.5 0-1.9-1-4-1s-2.5.9-4 .9c-1.7.1-3-1.7-4-3.1-2.7-3.9-3-8.5-1.3-11 1.2-1.8 3-2.8 4.7-2.8 1.8 0 2.9 1 4.3 1s2.3-1 4.4-1c1.6 0 3.2.9 4.4 2.3-3.9 2.2-3.3 7.7 1.1 9Z"/>
    </svg>
  `,
  linux: `
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 2.2c2.7 0 4.7 2.7 4.6 6.3 0 1.1.3 2.2 1 3.3l2.1 3.6c.9 1.6.1 3.6-1.7 4.1l-2.5.7c-1.1 1-2.2 1.5-3.5 1.5s-2.4-.5-3.5-1.5l-2.5-.7c-1.8-.5-2.6-2.5-1.7-4.1l2.1-3.6c.7-1.1 1-2.2 1-3.3C7.3 4.9 9.3 2.2 12 2.2Zm-2.3 7.2c.7 0 1.2-.7 1.2-1.5s-.5-1.5-1.2-1.5-1.2.7-1.2 1.5.5 1.5 1.2 1.5Zm4.6 0c.7 0 1.2-.7 1.2-1.5s-.5-1.5-1.2-1.5-1.2.7-1.2 1.5.5 1.5 1.2 1.5Zm-2.3 2.1c-1.7 0-3 1.3-3.4 3.3h6.8c-.4-2-1.7-3.3-3.4-3.3Z"/>
    </svg>
  `,
  unknown: `
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M11 3h2v10.2l3.6-3.6L18 11l-6 6-6-6 1.4-1.4 3.6 3.6V3Zm-6 16h14v2H5v-2Z"/>
    </svg>
  `,
};

// Asset matchers ordered by preference. The first matching asset wins.
// Patterns deliberately overlap with Tauri's default bundle names.
const ASSET_RULES = {
  windows: [
    { pattern: /\.msi$/i, label: 'MSI installer' },
    { pattern: /-setup\.exe$/i, label: 'NSIS installer' },
    { pattern: /\.exe$/i, label: 'Windows executable' },
  ],
  macos: [
    { pattern: /\.dmg$/i, label: 'DMG' },
    { pattern: /\.app\.tar\.gz$/i, label: 'macOS archive' },
  ],
  linux: [
    { pattern: /\.AppImage$/i, label: 'AppImage' },
    { pattern: /\.deb$/i, label: 'Debian package' },
    { pattern: /\.rpm$/i, label: 'RPM package' },
  ],
};

/** Detect the visitor's OS from userAgent + userAgentData. */
export function detectOs() {
  // userAgentData is the modern API, only on Chromium today; fall back to UA.
  const uaData = navigator.userAgentData;
  if (uaData?.platform) {
    const p = uaData.platform.toLowerCase();
    if (p.includes('win')) return 'windows';
    if (p.includes('mac')) return 'macos';
    if (p.includes('linux')) return 'linux';
  }
  const ua = (navigator.userAgent || '').toLowerCase();
  if (ua.includes('windows')) return 'windows';
  if (ua.includes('mac os') || ua.includes('macintosh')) return 'macos';
  if (ua.includes('linux') && !ua.includes('android')) return 'linux';
  return 'unknown';
}

export function osIcon(os = detectOs()) {
  return OS_ICONS[os] || OS_ICONS.unknown;
}

export function mountOsBadges(root = document) {
  const os = detectOs();
  root.querySelectorAll('[data-os-icon]').forEach((el) => {
    el.innerHTML = osIcon(os);
  });
  root.querySelectorAll('[data-os-name]').forEach((el) => {
    el.textContent = OS_LABELS[os];
  });
}

function readCache() {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed.fetchedAt !== 'number') return null;
    if (Date.now() - parsed.fetchedAt > CACHE_TTL_MS) return null;
    return parsed.release;
  } catch {
    return null;
  }
}

function writeCache(release) {
  try {
    localStorage.setItem(
      CACHE_KEY,
      JSON.stringify({ fetchedAt: Date.now(), release }),
    );
  } catch {
    // Storage unavailable / quota — non-fatal, just skip caching.
  }
}

/** Returns the latest release object from GitHub, or null on failure. */
async function fetchLatestRelease() {
  const cached = readCache();
  if (cached) return cached;
  const controller = new AbortController();
  const timeoutId = window.setTimeout(() => controller.abort(), 5000);
  try {
    const resp = await fetch(RELEASES_API, {
      headers: { Accept: 'application/vnd.github+json' },
      signal: controller.signal,
    });
    if (!resp.ok) return null;
    const release = await resp.json();
    writeCache(release);
    return release;
  } catch {
    return null;
  } finally {
    window.clearTimeout(timeoutId);
  }
}

/** Pick the best asset for the given OS, or null if none match. */
function assetForOs(release, os) {
  if (!release?.assets || !ASSET_RULES[os]) return null;
  for (const rule of ASSET_RULES[os]) {
    const match = release.assets.find((a) => rule.pattern.test(a.name));
    if (match) {
      return {
        url: match.browser_download_url,
        name: match.name,
        label: rule.label,
        size: match.size,
      };
    }
  }
  return null;
}

/** Render the smart download UI into the given mount point.
 *
 * The mount point should already contain a fallback link the visitor can
 * click immediately while we resolve the release in the background.
 */
export async function mountSmartDownload(mountEl) {
  if (!mountEl) return;
  const os = detectOs();
  const osLabel = OS_LABELS[os];

  // Optimistic state — show a button pointing at the releases page so even
  // a totally offline visitor (or someone GitHub is rate-limiting) can act.
  mountEl.innerHTML = `
    <a class="btn btn-primary is-loading" id="smart-download-primary" href="${RELEASES_PAGE}" rel="noopener" aria-busy="true">
      <span class="os-icon" aria-hidden="true">${osIcon(os)}</span>
      <span>Checking ${escapeHtml(osLabel)} build</span>
    </a>
    <a class="btn btn-ghost" href="${RELEASES_PAGE}" rel="noopener">All downloads</a>
  `;

  const release = await fetchLatestRelease();
  const primary = mountEl.querySelector('#smart-download-primary');
  primary?.classList.remove('is-loading');
  primary?.removeAttribute('aria-busy');

  if (!release) {
    if (primary) {
      primary.innerHTML = `<span class="os-icon" aria-hidden="true">${osIcon(os)}</span><span>Download for ${escapeHtml(osLabel)}</span>`;
    }
    return; // keep optimistic links
  }

  const asset = os !== 'unknown' ? assetForOs(release, os) : null;
  const versionLine = release.tag_name ? ` &middot; ${escapeHtml(release.tag_name)}` : '';

  if (asset) {
    primary.href = asset.url;
    primary.innerHTML = `<span class="os-icon" aria-hidden="true">${osIcon(os)}</span><span>Download for ${escapeHtml(osLabel)}${versionLine}</span>`;
    primary.title = asset.name;
  } else if (os !== 'unknown') {
    // We know their OS but the release lacks a matching asset (e.g. Linux
    // hasn't been built for this version yet). Send them to the page so they
    // can pick an alternative.
    primary.href = RELEASES_PAGE;
    primary.innerHTML = `<span class="os-icon" aria-hidden="true">${osIcon(os)}</span><span>${escapeHtml(osLabel)} build coming &middot; see all releases</span>`;
  } else {
    primary.href = RELEASES_PAGE;
    primary.innerHTML = `<span class="os-icon" aria-hidden="true">${osIcon(os)}</span><span>Download${versionLine}</span>`;
  }

  // Render a per-platform list so a visitor on the "wrong" OS (or sharing
  // a download link with someone else) can grab any binary.
  const platformList = ['windows', 'macos', 'linux']
    .map((target) => {
      const a = assetForOs(release, target);
      const label = OS_LABELS[target];
      if (a) {
        return `<li><a href="${a.url}" rel="noopener">${escapeHtml(label)} &mdash; ${escapeHtml(a.label)}</a></li>`;
      }
      return `<li style="color: var(--text-muted);">${escapeHtml(label)} &mdash; not yet published</li>`;
    })
    .join('');

  const moreId = 'smart-download-more';
  const more = document.getElementById(moreId);
  if (more) {
    more.innerHTML = `
      <p class="section-sub" style="margin-bottom: 14px;">
        Latest release: <strong>${escapeHtml(release.name || release.tag_name)}</strong>
        ${release.published_at ? ` &middot; ${escapeHtml(formatDate(release.published_at))}` : ''}
      </p>
      <ul style="list-style: none; padding: 0; margin: 0; display: grid; gap: 8px;">${platformList}</ul>
    `;
  }
}

function escapeHtml(s) {
  const d = document.createElement('div');
  d.textContent = s ?? '';
  return d.innerHTML;
}

function formatDate(iso) {
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  } catch {
    return iso;
  }
}
