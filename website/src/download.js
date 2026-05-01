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
  try {
    const resp = await fetch(RELEASES_API, {
      headers: { Accept: 'application/vnd.github+json' },
    });
    if (!resp.ok) return null;
    const release = await resp.json();
    writeCache(release);
    return release;
  } catch {
    return null;
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
    <a class="btn btn-primary" id="smart-download-primary" href="${RELEASES_PAGE}" rel="noopener">
      Download for ${escapeHtml(osLabel)}
    </a>
    <a class="btn btn-ghost" href="${RELEASES_PAGE}" rel="noopener">All downloads</a>
  `;

  const release = await fetchLatestRelease();
  if (!release) return; // keep optimistic links

  const asset = os !== 'unknown' ? assetForOs(release, os) : null;
  const primary = mountEl.querySelector('#smart-download-primary');
  const versionLine = release.tag_name ? ` &middot; ${escapeHtml(release.tag_name)}` : '';

  if (asset) {
    primary.href = asset.url;
    primary.innerHTML = `Download for ${escapeHtml(osLabel)}${versionLine}`;
    primary.title = asset.name;
  } else if (os !== 'unknown') {
    // We know their OS but the release lacks a matching asset (e.g. Linux
    // hasn't been built for this version yet). Send them to the page so they
    // can pick an alternative.
    primary.href = RELEASES_PAGE;
    primary.innerHTML = `${escapeHtml(osLabel)} build coming &middot; see all releases`;
  } else {
    primary.href = RELEASES_PAGE;
    primary.innerHTML = `Download${versionLine}`;
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
