import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Port 3000 is what the backend's CORS_ALLOWED_ORIGINS expects by default.
export default defineConfig({
  plugins: [react()],
  server: { port: 3000, strictPort: true },
  preview: { port: 3000, strictPort: true },
});
