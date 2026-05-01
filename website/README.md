# Audire website (audire.app)

Static marketing + legal site for [Audire](https://github.com/intelliholdings/audire). Required for Google OAuth brand verification.

## Local

```bash
cd website
npm install        # or bun install
npm run dev        # http://localhost:5173
npm run build      # output: website/dist
```

## What's where

| Page          | Path           | Public URL (after deploy) |
| ------------- | -------------- | ------------------------- |
| Home          | `index.html`   | `https://audire.app/`     |
| Privacy       | `privacy.html` | `https://audire.app/privacy` |
| Terms         | `terms.html`   | `https://audire.app/terms`   |

Production logo lives at `public/audire.svg`.

## Deploying

This is a vanilla static site. Any of these work with no extra config:

### Cloudflare Pages
1. Connect this repo, set root directory to `website/`.
2. Build command: `npm run build`. Output directory: `dist`.
3. Add `audire.app` as a custom domain. Cloudflare provisions HTTPS automatically.

### Vercel
1. Import the repo, set root directory to `website/`.
2. Framework preset: "Other". Build command + output dir same as above.
3. Add `audire.app` under Project Settings → Domains.

### Netlify
1. Same as Vercel. Build command + output dir same as above.

### GitHub Pages
Pages doesn't multi-page-route by default; use Cloudflare/Vercel/Netlify instead.

## Google OAuth verification checklist

Before submitting the consent screen for verification:

- [ ] Domain `audire.app` resolves over HTTPS.
- [ ] Privacy policy URL is reachable: `https://audire.app/privacy`.
- [ ] Terms of service URL is reachable: `https://audire.app/terms`.
- [ ] Logo at `public/audire.svg` is the final logo used by the site metadata and favicon.
- [ ] In Google Cloud Console → OAuth consent screen, set:
  - **Application home page**: `https://audire.app`
  - **Application privacy policy link**: `https://audire.app/privacy`
  - **Application terms of service link**: `https://audire.app/terms`
  - **Authorized domains**: `audire.app`
- [ ] Verify domain ownership in Google Search Console (one-time).
  - Paste the verification token into the `<meta name="google-site-verification" ...>` tag in `index.html`.

## Updating content

The pages are vanilla HTML. Edit them directly. The shared stylesheet is `src/style.css`. There is no JS framework, no build-time templating, no MDX — keep it that way unless you've got a strong reason. The whole point is that a Google reviewer can `view-source` and see exactly what's served.

## Contact

Updates to the privacy policy or terms of service should bump the "Last updated" date in the page itself and be committed with a clear commit message — that's the audit trail.
