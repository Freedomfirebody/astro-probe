const { spawn, execSync } = require('child_process');
const http = require('http');
const path = require('path');
const fs = require('fs');
const net = require('net');

const PROJECT_ROOT = path.resolve(__dirname, '..', '..');
const RUST_BINARY_NAME = process.platform === 'win32' ? 'astro-probe-server.exe' : 'astro-probe-server';
const RUST_BINARY_PATH_DEBUG = path.join(PROJECT_ROOT, 'target', 'debug', RUST_BINARY_NAME);
const RUST_BINARY_PATH_RELEASE = path.join(PROJECT_ROOT, 'target', 'release', RUST_BINARY_NAME);

let rustDaemonProcess = null;
let middleLayerProcess = null;
let testRunnerProcess = null;

// Helper to check and resolve free port sequentially
function getFreePort(startingPort) {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on('error', (err) => {
      if (err.code === 'EADDRINUSE') {
        resolve(getFreePort(startingPort + 1));
      } else {
        reject(err);
      }
    });
    server.listen(startingPort, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

// Helper to poll HTTP health endpoints
function waitForUrl(url, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const interval = setInterval(() => {
      if (Date.now() - start > timeoutMs) {
        clearInterval(interval);
        reject(new Error(`Timeout waiting for server at ${url}`));
        return;
      }

      http.get(url, (res) => {
        if (res.statusCode === 200) {
          clearInterval(interval);
          resolve();
        }
      }).on('error', () => {
        // Ignore error and retry
      });
    }, 500);
  });
}

// Ensure the Rust daemon is built
function ensureRustDaemonBuilt() {
  if (fs.existsSync(RUST_BINARY_PATH_DEBUG)) {
    console.log(`Using existing Rust debug binary: ${RUST_BINARY_PATH_DEBUG}`);
    return RUST_BINARY_PATH_DEBUG;
  }
  if (fs.existsSync(RUST_BINARY_PATH_RELEASE)) {
    console.log(`Using existing Rust release binary: ${RUST_BINARY_PATH_RELEASE}`);
    return RUST_BINARY_PATH_RELEASE;
  }

  console.log('Rust binary not found. Triggering cargo build...');
  try {
    execSync('cargo build --bin astro-probe-server', {
      cwd: PROJECT_ROOT,
      stdio: 'inherit'
    });
    console.log('Cargo build completed successfully.');
    return RUST_BINARY_PATH_DEBUG;
  } catch (error) {
    console.error('Failed to compile Rust daemon. Make sure cargo is in your PATH.');
    throw error;
  }
}

// Robust Cross-Platform Process Tree Killer
function killProcessTree(proc, name = 'Process') {
  if (!proc || !proc.pid) return;
  console.log(`Force terminating ${name} (PID: ${proc.pid}) and its children...`);
  if (process.platform === 'win32') {
    try {
      execSync(`taskkill /F /T /PID ${proc.pid}`, { stdio: 'ignore' });
    } catch (e) {
      // Process may already be dead
    }
  } else {
    try {
      // Send SIGKILL to the process group
      process.kill(-proc.pid, 'SIGKILL');
    } catch (e) {
      try {
        proc.kill('SIGKILL');
      } catch (err) {}
    }
  }
}

// Clean up processes on exit
function cleanup() {
  console.log('\nCleaning up processes...');
  if (middleLayerProcess) {
    killProcessTree(middleLayerProcess, 'Mock Middle Layer');
    middleLayerProcess = null;
  }
  if (rustDaemonProcess) {
    killProcessTree(rustDaemonProcess, 'Rust Daemon');
    rustDaemonProcess = null;
  }
  if (testRunnerProcess) {
    killProcessTree(testRunnerProcess, 'Test Runner');
    testRunnerProcess = null;
  }
}

async function main() {
  process.on('exit', cleanup);
  process.on('SIGINT', () => { process.exit(1); });
  process.on('SIGTERM', () => { process.exit(1); });
  process.on('uncaughtException', (err) => {
    console.error('Uncaught exception in E2E runner:', err);
    process.exit(1);
  });

  try {
    const binaryPath = ensureRustDaemonBuilt();

    // Clean up stale database files in the binary directory to ensure a fresh test environment
    const binaryDir = path.dirname(binaryPath);
    const dataDir = path.join(binaryDir, 'data');
    const cacheDb = path.join(binaryDir, 'astro-probe-cache.db');
    if (fs.existsSync(dataDir)) {
      console.log(`Cleaning up stale data directory: ${dataDir}`);
      try {
        fs.rmSync(dataDir, { recursive: true, force: true });
      } catch (err) {
        console.warn(`Warning: failed to clean up data directory: ${err.message}`);
      }
    }
    if (fs.existsSync(cacheDb)) {
      console.log(`Cleaning up stale cache database: ${cacheDb}`);
      try {
        fs.rmSync(cacheDb, { force: true });
      } catch (err) {
        console.warn(`Warning: failed to clean up cache database: ${err.message}`);
      }
    }

    // Resolve free ports dynamically to prevent conflicts
    console.log('Scanning for available ports...');
    const rustPort = await getFreePort(8080);
    const middleLayerPort = await getFreePort(3000);
    console.log(`Port resolution: Rust Daemon -> ${rustPort}, Mock Middle Layer -> ${middleLayerPort}`);

    // Start Rust Daemon on resolved port
    console.log('Starting Rust Daemon...');
    rustDaemonProcess = spawn(binaryPath, ['--port', String(rustPort)], {
      cwd: PROJECT_ROOT,
      stdio: 'inherit',
      detached: process.platform !== 'win32',
      env: {
        ...process.env,
        RUST_LOG: 'info',
        ASTRO_PROBE_IDLE_TIMEOUT_SECS: '5'
      }
    });

    rustDaemonProcess.on('error', (err) => {
      console.error('Failed to start Rust Daemon:', err);
      process.exit(1);
    });

    // Start Node.js Mock Middle Layer on resolved port
    console.log('Starting Mock Middle Layer Server...');
    middleLayerProcess = spawn('node', [path.join(__dirname, 'mock-middle-layer.js')], {
      cwd: __dirname,
      stdio: ['pipe', 'inherit', 'inherit'],
      detached: process.platform !== 'win32',
      env: {
        ...process.env,
        PORT: String(middleLayerPort),
        RUST_DAEMON_URL: `http://127.0.0.1:${rustPort}`,
        ASTRO_PROBE_IDLE_TIMEOUT_SECS: '5', // 5 seconds for testing auto-resume (minimum clamp)
        AUTO_EXIT_ON_PARENT_DEATH: 'true'   // enable zombie prevention
      }
    });

    middleLayerProcess.on('error', (err) => {
      console.error('Failed to start Mock Middle Layer Server:', err);
      process.exit(1);
    });

    // Wait for both health checks to pass
    console.log('Waiting for servers to become healthy...');
    await waitForUrl(`http://127.0.0.1:${rustPort}/health`);
    await waitForUrl(`http://127.0.0.1:${middleLayerPort}/health`);
    console.log('Both servers are healthy and ready.');

    // Run tests
    const testFilePath = path.join(__dirname, 'e2e.test.js');
    const negativeTestFilePath = path.join(__dirname, 'negative-tests.js');
    
    console.log('Executing E2E tests...');
    
    // Spawn standard test suite execution
    testRunnerProcess = spawn('node', [testFilePath], {
      cwd: __dirname,
      stdio: 'inherit',
      env: {
        ...process.env,
        MIDDLE_LAYER_URL: `http://127.0.0.1:${middleLayerPort}`
      }
    });

    testRunnerProcess.on('close', (code) => {
      console.log(`Standard test suite finished with exit code ${code}`);
      if (code !== 0) {
        process.exit(code);
      }

      // If standard tests pass, run negative tests
      console.log('Executing Negative E2E tests...');
      const negativeTestRunner = spawn('node', [negativeTestFilePath], {
        cwd: __dirname,
        stdio: 'inherit',
        env: {
          ...process.env,
          MIDDLE_LAYER_URL: `http://127.0.0.1:${middleLayerPort}`
        }
      });

      negativeTestRunner.on('close', (negCode) => {
        console.log(`Negative test suite finished with exit code ${negCode}`);
        process.exit(negCode);
      });
    });

  } catch (error) {
    console.error('E2E Test Runner Orchestration failed:', error);
    process.exit(1);
  }
}

main();
