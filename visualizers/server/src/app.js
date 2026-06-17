const express = require('express');
const cors = require('cors');
const path = require('path');
const fs = require('fs');
const symbolRouter = require('./routes/symbol');
const fileRouter = require('./routes/file');
const { setupProxy } = require('./routes/proxy');

const app = express();

// 1. Enable CORS for development
app.use(cors());

// 2. Health check
app.get('/health', (req, res) => res.send('OK'));

// 3. Register local symbol resolver route FIRST (Order-dependent routing)
app.use('/api/workspaces/:id/symbol', express.json(), symbolRouter);
app.use('/api/workspaces/:id/file', express.json(), fileRouter);

// 4. Register Rust Backend Proxy SECOND
setupProxy(app);

// 5. Serve Static Frontend files in Production
const frontendDist = path.resolve(__dirname, '../../frontend/dist');
if (fs.existsSync(frontendDist)) {
  app.use(express.static(frontendDist));
  // SPA Router Fallback
  app.get('*', (req, res) => {
    res.sendFile(path.join(frontendDist, 'index.html'));
  });
} else {
  app.get('*', (req, res) => {
    res.status(200).send('Astro-Probe Middle-Layer Server is running (Frontend assets not built yet).');
  });
}

module.exports = app;
