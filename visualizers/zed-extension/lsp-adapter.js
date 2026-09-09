const { 
  createConnection, 
  ProposedFeatures, 
  TextDocuments, 
  InitializeParams, 
  InitializeResult,
  ExecuteCommandParams
} = require('vscode-languageserver/node');
const { TextDocument } = require('vscode-languageserver-textdocument');
const axios = require('axios');
const open = require('open');
const url = require('url');

const BACKEND_URL = process.env.ASTRO_PROBE_URL || 'http://localhost:3000';
let workspaceRoot = process.argv[2]; // Passed from Wasm wrapper
const rootPath = workspaceRoot || "";

const connection = createConnection(ProposedFeatures.all);
const documents = new TextDocuments(TextDocument);

let activeWorkspaceId = null;

connection.onInitialize(async (params) => {
  if (params.rootUri) {
    try {
      workspaceRoot = url.fileURLToPath(params.rootUri);
    } catch (e) {
      connection.console.warn(`Failed to parse rootUri: ${e.message}`);
    }
  } else if (params.rootPath) {
    workspaceRoot = params.rootPath;
  }

  const rootPath = workspaceRoot || "";

  connection.console.log(`Astro-Probe LSP initialized for root: ${rootPath}`);
  
  // Register the workspace automatically with the backend
  if (!rootPath) {
    connection.console.warn("Workspace root is empty; skipping automatic workspace registration.");
  } else {
    try {
      const sanitizedRootPath = rootPath.replace(/[\\/]+$/, '');
      const workspaceName = sanitizedRootPath.split(/[\\/]/).pop() || 'zed-workspace';
      const response = await axios.post(`${BACKEND_URL}/api/workspaces`, {
        name: workspaceName,
        project_path: sanitizedRootPath
      });
      activeWorkspaceId = response.data.id;
      connection.console.log(`Successfully registered workspace. ID: ${activeWorkspaceId}`);
    } catch (err) {
      connection.console.error(`Failed to register workspace: ${err.message}`);
    }
  }

  const result = {
    capabilities: {
      executeCommandProvider: {
        commands: [
          'astro-probe.registerWorkspace',
          'astro-probe.triggerReanalysis',
          'astro-probe.openVisualizer',
          'astro-probe:register-workspace',
          'astro-probe:trigger-reanalysis',
          'astro-probe:open-visualizer'
        ]
      }
    }
  };
  return result;
});

// Handle custom editor commands triggered from Command Palette / Keymaps
connection.onExecuteCommand(async (params) => {
  const { command } = params;
  connection.console.log(`Executing LSP command: ${command}`);

  const rootPath = workspaceRoot || "";

  try {
    if (command === 'astro-probe.registerWorkspace' || command === 'astro-probe:register-workspace') {
      if (!rootPath) {
        throw new Error('Workspace root is empty; cannot register workspace.');
      }
      const sanitizedRootPath = rootPath.replace(/[\\/]+$/, '');
      const workspaceName = sanitizedRootPath.split(/[\\/]/).pop() || 'zed-workspace';
      const response = await axios.post(`${BACKEND_URL}/api/workspaces`, {
        name: workspaceName,
        project_path: sanitizedRootPath
      });
      activeWorkspaceId = response.data.id;
      connection.window.showInformationMessage(`Workspace registered with ID: ${activeWorkspaceId}`);
    } 
    else if (command === 'astro-probe.triggerReanalysis' || command === 'astro-probe:trigger-reanalysis') {
      if (!activeWorkspaceId) {
        throw new Error('No active workspace ID. Try registering the workspace first.');
      }
      connection.window.showInformationMessage('Triggering Astro-Probe static analysis...');
      await axios.post(`${BACKEND_URL}/api/workspaces/${activeWorkspaceId}/start`);
      connection.window.showInformationMessage('Astro-Probe analysis complete and database caches synchronized.');
    } 
    else if (command === 'astro-probe.openVisualizer' || command === 'astro-probe:open-visualizer') {
      const visualizerUrl = `${BACKEND_URL}/dashboard?workspaceId=${activeWorkspaceId || ''}`;
      connection.console.log(`Opening browser to visualizer URL: ${visualizerUrl}`);
      await open(visualizerUrl);
    }
  } catch (err) {
    connection.console.error(`Command execution failed: ${err.message}`);
    connection.window.showErrorMessage(`Astro-Probe command failed: ${err.message}`);
  }
});

documents.listen(connection);
connection.listen();
