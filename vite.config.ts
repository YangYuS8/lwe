import fs from 'node:fs';
import path from 'node:path';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const appRoot = process.cwd();
const fsAllow = [appRoot];

export default defineConfig({
  plugins: [sveltekit()],
  optimizeDeps: {
    include: ['@tauri-apps/api/core', '@tauri-apps/api/window']
  },
  server: {
    fs: {
      allow: fsAllow
    }
  }
});
