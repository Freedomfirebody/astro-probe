const express = require('express');
const axios = require('axios');
const fs = require('fs');
const path = require('path');

const app = express();
app.use(express.json());

// Port for this mock server
const PORT = process.env.PORT || 3000;
// Port where the Rust daemon is listening
const RUST_DAEMON_URL = process.env.RUST_DAEMON_URL || 'http://127.0.0.1:8080';

// -------------------------------------------------------------
// Zombie Prevention: Auto-Exit if Parent Process Dies
// -------------------------------------------------------------
if (process.env.AUTO_EXIT_ON_PARENT_DEATH === 'true') {
  // Gracefully exit if stdin closes (standard stream pipe broken)
  process.stdin.resume();
  process.stdin.on('end', () => {
    console.log('[Mock Middle Layer] Parent stdin closed. Exiting process...');
    process.exit(0);
  });

  // Polling heartbeat as a fallback check
  const parentCheckInterval = setInterval(() => {
    try {
      if (process.ppid === 1) {
        console.log('[Mock Middle Layer] Adopted by init (parent died). Exiting...');
        process.exit(0);
      }
      process.kill(process.ppid, 0); // Check parent existence
    } catch (e) {
      console.log('[Mock Middle Layer] Parent process no longer exists. Exiting...');
      process.exit(0);
    }
  }, 1000);
  parentCheckInterval.unref();
}

// Helper to get workspace project path from Rust daemon
async function getWorkspaceProjectPath(workspaceId) {
  try {
    const response = await axios.get(`${RUST_DAEMON_URL}/api/workspaces`);
    const workspaces = response.data;
    const ws = workspaces.find(w => w.id === workspaceId);
    return ws ? ws.project_path : null;
  } catch (error) {
    console.error('Error fetching workspaces from Rust daemon:', error.message);
    return null;
  }
}

// Lightweight symbol resolver for Java FQNs
function resolveJavaSymbol(projectPath, fqn) {
  // Parsing examples:
  // Class: com.example.simple.controller.UserController
  // Method: com.example.simple.controller.UserController.getUserById(java.lang.Long)
  // Parameter: com.example.simple.controller.UserController.getUserById(java.lang.Long)#id
  
  let classFqn = '';
  let methodName = '';
  let paramFqn = '';
  let isMethod = false;
  let isParam = false;

  const hashIdx = fqn.indexOf('#');
  let mainFqn = fqn;
  if (hashIdx !== -1) {
    paramFqn = fqn.substring(hashIdx + 1);
    mainFqn = fqn.substring(0, hashIdx);
    isParam = true;
  }

  const parenIdx = mainFqn.indexOf('(');
  if (parenIdx !== -1) {
    isMethod = true;
    const methodPart = mainFqn.substring(0, parenIdx);
    const lastDot = methodPart.lastIndexOf('.');
    classFqn = methodPart.substring(0, lastDot);
    methodName = methodPart.substring(lastDot + 1);
  } else {
    // Check if it has a method name without parentheses or just class
    // In Java, package names are lowercase, class names start with uppercase.
    // e.g. com.example.simple.controller.UserController.getUserById
    const parts = mainFqn.split('.');
    let lastClassIdx = -1;
    for (let i = 0; i < parts.length; i++) {
      if (parts[i] && parts[i][0] === parts[i][0].toUpperCase() && parts[i][0] !== '_') {
        lastClassIdx = i;
        break;
      }
    }
    if (lastClassIdx !== -1 && lastClassIdx < parts.length - 1) {
      classFqn = parts.slice(0, lastClassIdx + 1).join('.');
      methodName = parts.slice(lastClassIdx + 1).join('.');
      isMethod = true;
    } else {
      classFqn = mainFqn;
    }
  }

  // Convert class FQN to relative file path
  const relPath = path.join('src', 'main', 'java', ...classFqn.split('.')) + '.java';
  const filePath = path.resolve(projectPath, relPath);

  if (!fs.existsSync(filePath)) {
    throw new Error(`Source file not found at: ${filePath}`);
  }

  const content = fs.readFileSync(filePath, 'utf8');
  const lines = content.split(/\r?\n/);

  if (isParam) {
    // Find the method line first
    let methodLineIdx = -1;
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].includes(methodName) && lines[i].includes('(')) {
        methodLineIdx = i;
        break;
      }
    }

    if (methodLineIdx === -1) {
      throw new Error(`Method ${methodName} not found in ${filePath}`);
    }

    const lineContent = lines[methodLineIdx];
    const paramIdx = lineContent.indexOf(paramFqn);
    if (paramIdx !== -1) {
      return {
        filePath,
        startLine: methodLineIdx + 1,
        startColumn: paramIdx + 1,
        endLine: methodLineIdx + 1,
        endColumn: paramIdx + paramFqn.length + 1
      };
    }
    // Fallback to method line if parameter not found by exact name
    return {
      filePath,
      startLine: methodLineIdx + 1,
      startColumn: 1,
      endLine: methodLineIdx + 1,
      endColumn: lineContent.length + 1
    };
  }

  if (isMethod) {
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].includes(methodName) && lines[i].includes('(')) {
        const colIdx = lines[i].indexOf(methodName);
        return {
          filePath,
          startLine: i + 1,
          startColumn: colIdx + 1,
          endLine: i + 1,
          endColumn: colIdx + methodName.length + 1
        };
      }
    }
    throw new Error(`Method ${methodName} not found in ${filePath}`);
  }

  // Class definition resolution
  const className = classFqn.substring(classFqn.lastIndexOf('.') + 1);
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].includes(`class ${className}`) || lines[i].includes(`interface ${className}`)) {
      const colIdx = lines[i].indexOf(className);
      return {
        filePath,
        startLine: i + 1,
        startColumn: colIdx + 1,
        endLine: i + 1,
        endColumn: colIdx + className.length + 1
      };
    }
  }

  // Fallback to top of file
  return {
    filePath,
    startLine: 1,
    startColumn: 1,
    endLine: 1,
    endColumn: 1
  };
}

// 1. Symbol Resolution Endpoint (GET /api/workspaces/:id/symbol)
app.get('/api/workspaces/:id/symbol', async (req, res) => {
  const { id } = req.params;
  const { fqn } = req.query;

  if (!fqn) {
    return res.status(400).json({ error: 'Missing fqn query parameter' });
  }

  const projectPath = await getWorkspaceProjectPath(id);
  if (!projectPath) {
    return res.status(404).json({ error: `Workspace with ID ${id} not found` });
  }

  try {
    const location = resolveJavaSymbol(projectPath, fqn);
    res.json(location);
  } catch (error) {
    res.status(404).json({ error: error.message });
  }
});

// 1.5. File Retrieval Endpoint (GET /api/workspaces/:id/file)
const ALLOWED_EXTENSIONS = ['.java', '.xml', '.properties', '.yaml', '.yml', '.json', '.txt'];

app.get('/api/workspaces/:id/file', async (req, res) => {
  const { id } = req.params;
  const { filePath } = req.query;

  if (!filePath || typeof filePath !== 'string') {
    return res.status(400).json({ error: 'Invalid or missing parameter filePath' });
  }

  try {
    const projectPath = await getWorkspaceProjectPath(id);
    if (!projectPath) {
      return res.status(404).json({ error: `Workspace with ID ${id} not found` });
    }

    // Resolve projectPath to canonical/absolute path
    let absoluteProjectPath;
    try {
      absoluteProjectPath = fs.realpathSync(projectPath);
    } catch (err) {
      return res.status(404).json({ error: `Workspace project path not found on disk` });
    }

    // Resolve target path (it can be absolute or relative to projectPath)
    const resolvedPath = path.isAbsolute(filePath)
      ? filePath
      : path.resolve(absoluteProjectPath, filePath);

    // 1. Initial Path Traversal Check (Catching traversals even if file does not exist)
    const relativePre = path.relative(absoluteProjectPath, resolvedPath);
    if (relativePre === '' || relativePre.startsWith('..') || path.isAbsolute(relativePre)) {
      return res.status(403).json({ error: 'Access denied: Path traversal detected' });
    }

    // Restrict file extension check on resolvedPath pre-check to prevent traversal probes on unauthorized types
    const extPre = path.extname(resolvedPath).toLowerCase();
    if (!ALLOWED_EXTENSIONS.includes(extPre)) {
      return res.status(403).json({ error: 'Access denied: Unsupported file extension' });
    }

    // Get realpath of target file (requires file to exist)
    let absoluteFilePath;
    try {
      absoluteFilePath = fs.realpathSync(resolvedPath);
    } catch (err) {
      return res.status(404).json({ error: `File not found: ${filePath}` });
    }

    // 2. Canonical Path Traversal Check (Handling symbolic links pointing outside)
    const relativePost = path.relative(absoluteProjectPath, absoluteFilePath);
    if (relativePost === '' || relativePost.startsWith('..') || path.isAbsolute(relativePost)) {
      return res.status(403).json({ error: 'Access denied: Path traversal detected' });
    }

    // Double-check extension on resolved absolute path (following symlinks)
    const extPost = path.extname(absoluteFilePath).toLowerCase();
    if (!ALLOWED_EXTENSIONS.includes(extPost)) {
      return res.status(403).json({ error: 'Access denied: Unsupported file extension' });
    }

    // Get file stats asynchronously to verify size limit (< 2MB) and isFile
    const stats = await fs.promises.stat(absoluteFilePath);
    if (!stats.isFile()) {
      return res.status(400).json({ error: 'Requested path is not a file' });
    }
    if (stats.size > 2 * 1024 * 1024) {
      return res.status(403).json({ error: 'Access denied: File size exceeds 2MB limit' });
    }

    // Read and return the file content asynchronously
    const content = await fs.promises.readFile(absoluteFilePath, 'utf8');
    res.json({ content });
  } catch (error) {
    console.error(`[File Route Error] FAILED retrieving file '${filePath}':`, error.message);
    res.status(500).json({ error: error.message });
  }
});

// 2. Proxy Workspace Management and Graph Queries to Rust daemon
app.all('/api/workspaces*', async (req, res) => {
  const targetPath = req.params[0] || '';
  const url = `${RUST_DAEMON_URL}/api/workspaces${targetPath}`;
  
  try {
    const response = await axios({
      method: req.method,
      url,
      data: req.body,
      params: req.query,
      validateStatus: () => true // Allow passing through error statuses
    });
    res.status(response.status).json(response.data);
  } catch (error) {
    console.error(`Error forwarding request to Rust daemon (${url}):`, error.message);
    res.status(500).json({ error: 'Failed to proxy request to Rust backend' });
  }
});

// Health check endpoint
app.get('/health', (req, res) => {
  res.send('OK');
});

// Start the server
const server = app.listen(PORT, () => {
  console.log(`Mock Middle Layer Server running on port ${PORT}`);
});

// Handle termination signals
process.on('SIGTERM', () => {
  server.close(() => {
    console.log('Mock Middle Layer Server terminated');
  });
});
