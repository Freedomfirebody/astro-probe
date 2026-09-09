require('dotenv').config();
const http = require('http');
const app = require('./app');

const PORT = process.env.PORT || 3000;
const server = http.createServer(app);

server.listen(PORT, () => {
  console.log(`[Middle Layer] Server is running on port ${PORT}`);
  console.log(`[Middle Layer] Rust backend URL: ${process.env.RUST_BACKEND_URL || 'http://localhost:8080'}`);
});

// Handle graceful termination
const gracefulShutdown = () => {
  console.log('[Middle Layer] Received termination signal. Shutting down gracefully...');
  server.close(() => {
    console.log('[Middle Layer] HTTP server closed.');
    process.exit(0);
  });
};

process.on('SIGTERM', gracefulShutdown);
process.on('SIGINT', gracefulShutdown);
