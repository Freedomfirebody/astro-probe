import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import fs from 'fs';
import path from 'path';

const serveMonacoDev = () => ({
  name: 'serve-monaco-dev',
  configureServer(server: any) {
    server.middlewares.use((req: any, res: any, next: any) => {
      if (req.url && req.url.startsWith('/monaco/vs/')) {
        const relativePath = req.url.slice('/monaco/vs/'.length);
        const cleanPath = relativePath.split('?')[0];
        const localPath = path.resolve(__dirname, 'node_modules/monaco-editor/min/vs', cleanPath);
        if (fs.existsSync(localPath) && fs.statSync(localPath).isFile()) {
          const ext = path.extname(cleanPath);
          let contentType = 'application/javascript';
          if (ext === '.css') contentType = 'text/css';
          res.setHeader('Content-Type', contentType);
          res.end(fs.readFileSync(localPath));
          return;
        }
      }
      next();
    });
  }
});

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react(), serveMonacoDev()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:3000',
        changeOrigin: true,
      },
    },
  },
});
