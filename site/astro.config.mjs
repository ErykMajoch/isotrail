import { defineConfig } from 'astro/config';

export default defineConfig({
  output: 'static',
  // The Content-Security-Policy in public/_headers allows no inline styles
  build: { inlineStylesheets: 'never' },
  devToolbar: { enabled: false },
});
