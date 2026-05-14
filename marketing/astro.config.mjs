import { defineConfig } from 'astro/config';

// https://astro.build/config
export default defineConfig({
  site: 'https://feedbackmonk.com',
  trailingSlash: 'ignore',
  build: {
    format: 'directory',
  },
  server: {
    port: 14210,
    host: '127.0.0.1',
  },
  vite: {
    server: {
      strictPort: true,
    },
    preview: {
      strictPort: true,
    },
  },
});
