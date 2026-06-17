const { createProxyMiddleware } = require('http-proxy-middleware');

function setupProxy(app) {
  const rustBackendUrl = process.env.RUST_BACKEND_URL || 'http://127.0.0.1:8080';

  const proxyOptions = {
    target: rustBackendUrl,
    changeOrigin: true,
    ws: true,
    onError: (err, req, res) => {
      console.error(`[Proxy Error] failed to proxy to Rust backend (${rustBackendUrl}):`, err.message);
      if (!res.headersSent) {
        res.status(502).json({ error: 'Gateway Error: Failed to communicate with backend' });
      }
    }
  };

  // Match all /api/workspaces paths
  app.use('/api/workspaces', createProxyMiddleware(proxyOptions));
}

module.exports = { setupProxy };
