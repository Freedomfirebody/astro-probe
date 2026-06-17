const axios = require('axios');
const fs = require('fs');
const path = require('path');
const assert = require('assert');

const MIDDLE_LAYER_URL = process.env.MIDDLE_LAYER_URL || 'http://127.0.0.1:3000';
const PROJECT_ROOT = path.resolve(__dirname, '..', '..');
const SIMPLE_SPRING_PATH = path.resolve(PROJECT_ROOT, 'test-samples', 'simple-spring');

const tests = [];
function test(name, fn) {
  tests.push({ name, fn });
}

let activeWorkspaceId = null;

// Bootstrap workspace to test operations on active workspace
test('Setup: Create Valid Workspace for Negative Tests', async () => {
  const payload = {
    name: 'negative-test-workspace',
    project_path: SIMPLE_SPRING_PATH
  };
  const response = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, payload);
  assert.strictEqual(response.status, 201);
  activeWorkspaceId = response.data.id;
  console.log(`   Workspace created with ID: ${activeWorkspaceId}`);
});

// ==========================================
// TIER 1: INPUT VALIDATION & MALFORMED PAYLOADS
// ==========================================

test('Negative 1.1: Create Workspace with Malformed JSON syntax', async () => {
  try {
    await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, '{ "name": "bad", "project_path": ', {
      headers: { 'Content-Type': 'application/json' }
    });
    throw new Error('Expected malformed JSON payload to return 400 Bad Request');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 400, `Expected status 400, got ${error.response.status}`);
  }
});

test('Negative 1.2: Create Workspace with Missing Required Fields', async () => {
  try {
    await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, {
      name: 'missing-path'
    });
    throw new Error('Expected missing project_path to return 400 Bad Request');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 400, `Expected status 400, got ${error.response.status}`);
  }
});

test('Negative 1.3: Create Workspace with Empty name', async () => {
  try {
    await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, {
      name: '',
      project_path: SIMPLE_SPRING_PATH
    });
    throw new Error('Expected empty name to return 400 Bad Request');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 400, `Expected status 400, got ${error.response.status}`);
  }
});

// ==========================================
// TIER 2: INVALID WORKSPACE IDENTIFIERS
// ==========================================

test('Negative 2.1: Call Graph Query on Invalid Workspace ID', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/invalid-uuid-1234/call-graph`, {
      params: { method: 'UserController.getUserById', direction: 'outgoing' }
    });
    throw new Error('Expected query to fail on invalid workspace ID');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

test('Negative 2.2: Lineage Query on Invalid Workspace ID', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/invalid-uuid-1234/lineage`, {
      params: { node: 'UserController.getUserById#id', direction: 'downstream' }
    });
    throw new Error('Expected query to fail on invalid workspace ID');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

test('Negative 2.3: Route Query on Invalid Workspace ID', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/invalid-uuid-1234/routes`);
    throw new Error('Expected query to fail on invalid workspace ID');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

test('Negative 2.4: Symbol Query on Invalid Workspace ID', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/invalid-uuid-1234/symbol`, {
      params: { fqn: 'com.example.simple.controller.UserController' }
    });
    throw new Error('Expected query to fail on invalid workspace ID');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

test('Negative 2.5: File Retrieval on Invalid Workspace ID', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/invalid-uuid-1234/file`, {
      params: { filePath: 'src/main/java/com/example/simple/controller/UserController.java' }
    });
    throw new Error('Expected query to fail on invalid workspace ID');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

// ==========================================
// TIER 3: FILE RETRIEVAL BOUNDARY & ATTACK SCENARIOS
// ==========================================

test('Negative 3.1: File Retrieval - Non-existent file', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/file`, {
      params: { filePath: 'src/main/java/NotExist.java' }
    });
    throw new Error('Expected 404 for non-existent file path');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

test('Negative 3.2: File Retrieval - Path Traversal Probe (Relative)', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/file`, {
      params: { filePath: '../../Cargo.toml' }
    });
    throw new Error('Expected 403 Forbidden for path traversal');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 403, `Expected status 403, got ${error.response.status}`);
  }
});

test('Negative 3.3: File Retrieval - Path Traversal Probe (Absolute)', async () => {
  try {
    const targetFile = process.platform === 'win32' ? 'C:\\Windows\\win.ini' : '/etc/passwd';
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/file`, {
      params: { filePath: targetFile }
    });
    throw new Error('Expected 403 Forbidden for path traversal');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 403, `Expected status 403, got ${error.response.status}`);
  }
});

test('Negative 3.4: File Retrieval - Unsupported File Extension', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/file`, {
      params: { filePath: 'pom.xml.bak' }
    });
    throw new Error('Expected 403 Forbidden for unsupported file extension');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 403, `Expected status 403, got ${error.response.status}`);
  }
});

test('Negative 3.5: File Retrieval - Directory Path Request', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/file`, {
      params: { filePath: 'src/main/java' }
    });
    throw new Error('Expected 400 Bad Request or 403 for directory target');
  } catch (error) {
    if (!error.response) throw error;
    assert.ok([400, 403, 404].includes(error.response.status), `Expected status 400/403/404, got ${error.response.status}`);
  }
});

test('Negative 3.6: File Retrieval - File Exceeding 2MB Limit', async () => {
  const hugeFilePath = path.resolve(SIMPLE_SPRING_PATH, 'src', 'main', 'java', 'com', 'example', 'simple', 'controller', 'HugeUserController.java');
  const size = 2.1 * 1024 * 1024; // > 2MB
  const buffer = Buffer.alloc(size, 'A');
  fs.writeFileSync(hugeFilePath, buffer);

  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/file`, {
      params: { filePath: 'src/main/java/com/example/simple/controller/HugeUserController.java' }
    });
    throw new Error('Expected 403 Forbidden for file > 2MB');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 403, `Expected status 403, got ${error.response.status}`);
  } finally {
    if (fs.existsSync(hugeFilePath)) {
      fs.unlinkSync(hugeFilePath);
    }
  }
});

test('Negative 3.7: File Retrieval - Array Injection on filePath Parameter', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/file?filePath=a&filePath=b`);
    throw new Error('Expected array parameter to fail gracefully');
  } catch (error) {
    if (!error.response) throw error;
    assert.ok([400, 403, 500].includes(error.response.status), `Expected 400, 403 or 500, got ${error.response.status}`);
  }
});

// ==========================================
// TIER 4: SYMBOL RESOLUTION CORNER CASES
// ==========================================

test('Negative 4.1: Symbol Resolution - Missing FQN Parameter', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`);
    throw new Error('Expected 400 Bad Request for missing fqn');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 400, `Expected status 400, got ${error.response.status}`);
  }
});

test('Negative 4.2: Symbol Resolution - Non-existent Class', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`, {
      params: { fqn: 'com.example.simple.controller.NonExistentController' }
    });
    throw new Error('Expected 404 Not Found for non-existent class');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

test('Negative 4.3: Symbol Resolution - Non-existent Method', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`, {
      params: { fqn: 'com.example.simple.controller.UserController.nonExistentMethod()' }
    });
    throw new Error('Expected 404 Not Found for non-existent method');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

test('Negative 4.4: Symbol Resolution - Malformed FQN structure', async () => {
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`, {
      params: { fqn: 'malformed_fqn_without_dots' }
    });
    throw new Error('Expected 404 Not Found for malformed FQN structure');
  } catch (error) {
    if (!error.response) throw error;
    assert.strictEqual(error.response.status, 404, `Expected status 404, got ${error.response.status}`);
  }
});

// ==========================================
// TEARDOWN
// ==========================================

test('Teardown: Delete Workspace', async () => {
  if (activeWorkspaceId) {
    const deleteResp = await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}`);
    assert.strictEqual(deleteResp.status, 200);
    assert.strictEqual(deleteResp.data.success, true);
  }
});

// Executability
async function runTests() {
  console.log('Running Negative E2E tests...\n');
  let passedCount = 0;
  let failedCount = 0;
  const startTime = Date.now();

  for (const t of tests) {
    console.log(`[RUN] ${t.name}`);
    try {
      await t.fn();
      console.log(`[PASS] ${t.name}\n`);
      passedCount++;
    } catch (err) {
      console.error(`[FAIL] ${t.name}`);
      console.error(`       Error: ${err.stack || err.message}\n`);
      failedCount++;
    }
  }

  const duration = ((Date.now() - startTime) / 1000).toFixed(2);
  console.log('==================================================');
  console.log(`NEGATIVE E2E TEST RUN SUMMARY:`);
  console.log(`  Passed: ${passedCount}`);
  console.log(`  Failed: ${failedCount}`);
  console.log(`  Total:  ${tests.length}`);
  console.log(`  Time:   ${duration}s`);
  console.log('==================================================');

  if (failedCount > 0) {
    process.exit(1);
  } else {
    process.exit(0);
  }
}

if (require.main === module) {
  runTests();
}

module.exports = { runTests };
