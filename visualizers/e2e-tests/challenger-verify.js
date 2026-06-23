const assert = require('assert');
const { fileURLToPath } = require('url');

// Mock frontend URI generation logic from CodeViewer.tsx
function generateZedUri(filePath, line, col) {
  if (!filePath) return null;
  const normalizedPath = filePath.replace(/\\/g, '/');
  const encodedPath = encodeURIComponent(normalizedPath)
    .replace(/%2F/g, '/')
    .replace(/%3A/g, ':');
  return `zed://file/${encodedPath}:${line}:${col}`;
}

// Robust URI parser representing how Zed/OS handler should decode
function parseZedUri(zedUri) {
  if (!zedUri.startsWith('zed://file/')) {
    throw new Error('URI must start with zed://file/');
  }
  const mainPart = zedUri.substring('zed://file/'.length);
  
  // Find the last two colon-separated segments for line and column
  const lastColon = mainPart.lastIndexOf(':');
  if (lastColon === -1) throw new Error('Missing line/col suffix');
  const secondLastColon = mainPart.lastIndexOf(':', lastColon - 1);
  if (secondLastColon === -1) throw new Error('Missing line/col suffix');

  const encodedPath = mainPart.substring(0, secondLastColon);
  const lineStr = mainPart.substring(secondLastColon + 1, lastColon);
  const colStr = mainPart.substring(lastColon + 1);

  // Decode percent-encoded characters in path (e.g. spaces %20)
  const filePath = decodeURIComponent(encodedPath);
  const line = parseInt(lineStr, 10);
  const col = parseInt(colStr, 10);

  return { filePath, line, col };
}

// Mock LSP adapter rootUri resolution from lsp-adapter.js
function resolveWorkspaceRoot(params) {
  let workspaceRoot = null;
  if (params.rootUri) {
    try {
      workspaceRoot = fileURLToPath(params.rootUri);
    } catch (e) {
      // Mock error handling
      workspaceRoot = `ERROR: ${e.message}`;
    }
  } else if (params.rootPath) {
    workspaceRoot = params.rootPath;
  }
  return workspaceRoot;
}

// -------------------------------------------------------------
// Test Case 1: Deep-linking URI Generation and Parsing
// -------------------------------------------------------------
function testDeepLinking() {
  console.log('Running Test Case 1: Deep-linking URI Parsing...');
  
  const cases = [
    {
      description: 'Standard Windows Path',
      originalPath: 'D:\\project\\src\\Main.java',
      line: 24,
      col: 8,
      expectedUri: 'zed://file/D:/project/src/Main.java:24:8'
    },
    {
      description: 'Standard Unix Path',
      originalPath: '/home/user/project/src/Main.java',
      line: 42,
      col: 1,
      expectedUri: 'zed://file//home/user/project/src/Main.java:42:1'
    },
    {
      description: 'Path with Spaces (Windows)',
      originalPath: 'C:\\My Projects\\src\\Main.java',
      line: 10,
      col: 5,
      expectedUri: 'zed://file/C:/My%20Projects/src/Main.java:10:5'
    },
    {
      description: 'Path with Spaces (Unix)',
      originalPath: '/home/user/my projects/Main.java',
      line: 99,
      col: 12,
      expectedUri: 'zed://file//home/user/my%20projects/Main.java:99:12'
    },
    {
      description: 'Path with Special Characters (Unix)',
      originalPath: '/home/user/project#3/Main.java',
      line: 1,
      col: 1,
      expectedUri: 'zed://file//home/user/project%233/Main.java:1:1'
    }
  ];

  for (const tc of cases) {
    console.log(`  - Testing: ${tc.description}`);
    const generated = generateZedUri(tc.originalPath, tc.line, tc.col);
    assert.strictEqual(generated, tc.expectedUri, `URI generation failed: expected ${tc.expectedUri}, got ${generated}`);
    
    // Round trip parsing
    const parsed = parseZedUri(generated);
    // On Windows, the original path has backslashes. Let's normalize backslashes for comparison.
    const normalizedOriginal = tc.originalPath.replace(/\\/g, '/');
    const normalizedParsed = parsed.filePath.replace(/\\/g, '/');
    assert.strictEqual(normalizedParsed, normalizedOriginal, `Path mismatch: expected ${normalizedOriginal}, got ${normalizedParsed}`);
    assert.strictEqual(parsed.line, tc.line, `Line mismatch: expected ${tc.line}, got ${parsed.line}`);
    assert.strictEqual(parsed.col, tc.col, `Col mismatch: expected ${tc.col}, got ${parsed.col}`);
  }
  console.log('✅ Test Case 1 Passed successfully!');
}

// -------------------------------------------------------------
// Test Case 2: Workspace Path Resolution & Payload verification
// -------------------------------------------------------------
function testWorkspaceRegistration() {
  console.log('Running Test Case 2: Workspace Registration Paths...');
  
  // We can write validation for the registration request format.
  const payload = {
    name: 'test-workspace',
    project_path: 'D:\\project\\rust\\astro-probe'
  };

  assert.ok(payload.name, 'Name must not be empty');
  assert.ok(payload.project_path, 'Project path must not be empty');
  
  // Verify trailing slash removal (similar to workspace root name extraction in lsp-adapter.js line 37)
  const getWorkspaceNameFromPath = (p) => p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || 'zed-workspace';
  
  assert.strictEqual(getWorkspaceNameFromPath('D:\\project\\rust\\astro-probe\\'), 'astro-probe');
  assert.strictEqual(getWorkspaceNameFromPath('/home/user/my-project/'), 'my-project');
  assert.strictEqual(getWorkspaceNameFromPath('C:/'), 'C:');
  
  console.log('✅ Test Case 2 Passed successfully!');
}

// -------------------------------------------------------------
// Test Case 3: LSP Adapter rootUri -> Absolute Path Conversion
// -------------------------------------------------------------
function testLspAdapterInit() {
  console.log('Running Test Case 3: LSP Adapter rootUri parsing...');

  const winUri = 'file:///D:/project/rust/astro-probe/test-samples/simple-spring';
  const unixUri = 'file:///home/user/project/test-samples/simple-spring';
  const encodedUri = 'file:///D:/project%20space/simple-spring';

  // We want to verify that Node.js fileURLToPath parses these correctly based on platform/standard.
  // Note: fileURLToPath behavior depends on the host OS for drive letters.
  // For cross-platform safety, we test that it behaves correctly for standard URIs.
  
  if (process.platform === 'win32') {
    const resolvedWin = resolveWorkspaceRoot({ rootUri: winUri });
    assert.strictEqual(resolvedWin.toLowerCase(), 'd:\\project\\rust\\astro-probe\\test-samples\\simple-spring');
    
    const resolvedEncoded = resolveWorkspaceRoot({ rootUri: encodedUri });
    assert.strictEqual(resolvedEncoded.toLowerCase(), 'd:\\project space\\simple-spring');
  } else {
    const resolvedUnix = resolveWorkspaceRoot({ rootUri: unixUri });
    assert.strictEqual(resolvedUnix, '/home/user/project/test-samples/simple-spring');
  }

  // Test rootPath fallback
  const resolvedPath = resolveWorkspaceRoot({ rootPath: '/etc/configs' });
  assert.strictEqual(resolvedPath, '/etc/configs');

  // Test failure case (invalid URI scheme)
  const invalidUri = 'http://localhost:3000/some/path';
  const resolvedInvalid = resolveWorkspaceRoot({ rootUri: invalidUri });
  assert.ok(resolvedInvalid.startsWith('ERROR:'), 'Expected error on invalid rootUri scheme');

  console.log('✅ Test Case 3 Passed successfully!');
}

// Run all test cases
try {
  testDeepLinking();
  console.log('');
  testWorkspaceRegistration();
  console.log('');
  testLspAdapterInit();
  console.log('\nAll empirical tests PASSED successfully!');
} catch (err) {
  console.error('\n❌ Test execution FAILED:', err.message);
  process.exit(1);
}
