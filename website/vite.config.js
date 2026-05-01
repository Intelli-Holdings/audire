import { defineConfig } from 'vite';
import { resolve } from 'path';

// Multi-page static build: each top-level HTML file becomes its own page.
// The legal pages are required for Google's OAuth brand verification:
//   - index.html       application home page
//   - privacy.html     privacy policy
//   - terms.html       terms of service
//   - pricing.html     pricing page
export default defineConfig({
  build: {
    rollupOptions: {
      input: {
        index: resolve(__dirname, 'index.html'),
        privacy: resolve(__dirname, 'privacy.html'),
        terms: resolve(__dirname, 'terms.html'),
        pricing: resolve(__dirname, 'pricing.html'),
      },
    },
  },
});
