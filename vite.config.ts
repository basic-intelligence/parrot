/// <reference types="node" />

import { defineConfig } from 'vite';
import { resolve } from 'node:path';

export default defineConfig({
  clearScreen: false,
  server: {
    strictPort: true,
    port: 1420,
  },
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        recording: resolve(__dirname, 'recording.html'),
      },
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
});
