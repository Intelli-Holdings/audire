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
| Home          | `index.html`   | `https://audire-chi.vercel.app/`     |
| Privacy       | `privacy.html` | `https://audire-chi.vercel.app/privacy` |
| Terms         | `terms.html`   | `https://audire-chi.vercel.app/terms`   |

Production logo lives at `public/audire.svg`.

## Deploying

This is a vanilla static site. Any of these work with no extra config:

### Cloudflare Pages
1. Connect this repo, set root directory to `website/`.
2. Build command: `npm run build`. Output directory: `dist`.
3. Add `audire.app` as a custom domain. Cloudflare provisions HTTPS automatically.