const axios = require('axios');
const fs = require('fs');
const path = require('path');

const MIDDLE_LAYER_URL = process.env.MIDDLE_LAYER_URL || 'http://127.0.0.1:3000';
const PROJECT_ROOT = path.resolve(__dirname, '..', '..');
const SIMPLE_SPRING_PATH = path.resolve(PROJECT_ROOT, 'test-samples', 'simple-spring');
const COMPLEX_SPRING_PATH = path.resolve(PROJECT_ROOT, 'test-samples', 'complex-spring');

const tests = [];
function test(name, fn) {
  tests.push({ name, fn });
}

// Global state shared between tests
let activeWorkspaceId = null;
let activeDbPath = null;
let complexWorkspaceId = null;
let complexDbPath = null;

// ==========================================
// TIER 1: FEATURE COVERAGE
// ==========================================

test('Tier 1.1: Workspace Management - Create Workspace', async () => {
  // Pre-cleanup workspaces of the same names to ensure clean state
  try {
    const listRes = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces`);
    for (const ws of listRes.data) {
      if (ws.name === 'e2e-simple-spring' || ws.name === 'e2e-complex-spring') {
        await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${ws.id}`);
      }
    }
  } catch (err) {
    // Ignore if server is not running or fresh
  }

  const payload = {
    name: 'e2e-simple-spring',
    project_path: SIMPLE_SPRING_PATH
  };

  const response = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, payload);
  if (response.status !== 201) {
    throw new Error(`Expected status 201, got ${response.status}`);
  }

  const ws = response.data;
  if (!ws.id || ws.name !== payload.name || ws.status !== 'loaded') {
    throw new Error(`Invalid workspace payload returned: ${JSON.stringify(ws)}`);
  }

  activeWorkspaceId = ws.id;
  activeDbPath = ws.db_path;
  console.log(`   Workspace created with ID: ${activeWorkspaceId}`);
});

test('Tier 1.2: Workspace Management - List Workspaces', async () => {
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces`);
  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const list = response.data;
  const found = list.find(w => w.id === activeWorkspaceId);
  if (!found) {
    throw new Error(`Created workspace ${activeWorkspaceId} not found in list: ${JSON.stringify(list)}`);
  }
  if (found.status !== 'loaded') {
    throw new Error(`Expected workspace status to be 'loaded', got '${found.status}'`);
  }
});

test('Tier 1.3: Workspace Management - Stop Workspace', async () => {
  const response = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/stop`);
  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const ws = response.data;
  if (ws.status !== 'unloaded') {
    throw new Error(`Expected workspace status to transition to 'unloaded', got '${ws.status}'`);
  }
});

test('Tier 1.4: Workspace Management - Start Workspace', async () => {
  const response = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/start`);
  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const ws = response.data;
  if (ws.status !== 'loaded') {
    throw new Error(`Expected workspace status to transition to 'loaded', got '${ws.status}'`);
  }
});

test('Tier 1.5: Integrations - Query Call Graph (Outgoing)', async () => {
  // Query outgoing calls from UserController.getUserById
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/call-graph`, {
    params: {
      method: 'UserController.getUserById',
      direction: 'outgoing'
    }
  });

  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const { edges } = response.data;
  if (!Array.isArray(edges) || edges.length === 0) {
    throw new Error('Expected at least one call edge originating from UserController.getUserById');
  }

  const hasTarget = edges.some(e => e.callee.includes('UserService') && e.callee.includes('findById'));
  if (!hasTarget) {
    throw new Error(`Call graph missing callee 'UserService.findById'. Edges: ${JSON.stringify(edges)}`);
  }
});

test('Tier 1.6: Integrations - Query Lineage (Downstream)', async () => {
  // Query lineage downstream from id parameter in UserController.getUserById(java.lang.Long)
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/lineage`, {
    params: {
      node: 'UserController.getUserById#id',
      direction: 'downstream'
    }
  });

  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const { nodes, edges } = response.data;
  if (!Array.isArray(nodes) || nodes.length === 0) {
    throw new Error('Lineage response should contain nodes');
  }

  // Verify that it flows down to the Service method param
  const hasServiceNode = nodes.some(n => n.includes('UserService') && n.includes('findById') && n.includes('#id'));
  if (!hasServiceNode) {
    throw new Error(`Lineage graph does not propagate to UserService.findById#id. Nodes: ${JSON.stringify(nodes)}`);
  }
});

test('Tier 1.7: Integrations - Query Web Routes', async () => {
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/routes`);
  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const { routes } = response.data;
  if (!Array.isArray(routes) || routes.length === 0) {
    throw new Error('Expected routes list to be non-empty');
  }

  const hasUserGet = routes.some(r => r.http_method === 'GET' && r.path === '/api/users/{id}');
  if (!hasUserGet) {
    throw new Error(`Expected to find GET /api/users/{id} route, got: ${JSON.stringify(routes)}`);
  }
});

test('Tier 1.8: Integrations - Monaco Code Viewer Symbol Resolution', async () => {
  const fqn = 'com.example.simple.controller.UserController.getUserById(java.lang.Long)';
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`, {
    params: { fqn }
  });

  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const loc = response.data;
  if (!loc.filePath || typeof loc.startLine !== 'number' || typeof loc.startColumn !== 'number') {
    throw new Error(`Invalid symbol location payload: ${JSON.stringify(loc)}`);
  }

  if (!loc.filePath.endsWith('UserController.java')) {
    throw new Error(`Expected filepath to end with UserController.java, got ${loc.filePath}`);
  }

  // Read the source file and verify line contents contains our method name
  const content = fs.readFileSync(loc.filePath, 'utf8');
  const lines = content.split(/\r?\n/);
  const targetLine = lines[loc.startLine - 1];

  if (!targetLine.includes('getUserById')) {
    throw new Error(`Line ${loc.startLine} does not contain method name 'getUserById': '${targetLine}'`);
  }
});

test('Tier 1.9: Integrations - Zed Deep Link Generation', async () => {
  const fqn = 'com.example.simple.controller.UserController.getUserById(java.lang.Long)#id';
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`, {
    params: { fqn }
  });

  const loc = response.data;
  
  // Simulate Zed link generation: zed://file/<project_path>/<rel_path>:<line>:<col>
  const workspacePath = SIMPLE_SPRING_PATH;
  const relativeFile = path.relative(workspacePath, loc.filePath).replace(/\\/g, '/');
  
  const zedUri = `zed://file/${workspacePath.replace(/\\/g, '/')}/${relativeFile}:${loc.startLine}:${loc.startColumn}`;
  console.log(`   Generated Zed URI: ${zedUri}`);

  // Assert correct URI format
  if (!zedUri.startsWith('zed://file/')) {
    throw new Error('Zed URI must start with zed://file/');
  }
  const parts = zedUri.substring('zed://file/'.length).split(':');
  if (parts.length < 3) {
    throw new Error('Zed URI must contain line and column suffix');
  }
});


// ==========================================
// TIER 2: BOUNDARY & CORNER CASES
// ==========================================

test('Tier 2.1: Workspace Boundaries - Create with Non-existent path', async () => {
  const payload = {
    name: 'non-existent',
    project_path: path.join(PROJECT_ROOT, 'test-samples', 'non-existent-directory')
  };

  try {
    await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, payload);
    throw new Error('Expected request to fail with 400 or 500');
  } catch (error) {
    if (!error.response) throw error;
    if (error.response.status !== 400 && error.response.status !== 500) {
      throw new Error(`Expected status 400 or 500, got ${error.response.status}`);
    }
  }
});

test('Tier 2.2: Workspace Boundaries - Create with Empty name', async () => {
  const payload = {
    name: '',
    project_path: SIMPLE_SPRING_PATH
  };

  try {
    await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, payload);
    throw new Error('Expected request to fail with 400');
  } catch (error) {
    if (!error.response) throw error;
    if (error.response.status !== 400) {
      throw new Error(`Expected status 400, got ${error.response.status}`);
    }
  }
});

test('Tier 2.3: Workspace Boundaries - Operations on Non-existent ID', async () => {
  const fakeId = '00000000-0000-0000-0000-000000000000';

  // Stop fake workspace
  try {
    await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces/${fakeId}/stop`);
    throw new Error('Expected stop to return 404');
  } catch (error) {
    if (!error.response || error.response.status !== 404) {
      throw new Error(`Expected stop to return 404, got ${error.response ? error.response.status : error.message}`);
    }
  }

  // Start fake workspace
  try {
    await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces/${fakeId}/start`);
    throw new Error('Expected start to return 404');
  } catch (error) {
    if (!error.response || error.response.status !== 404) {
      throw new Error(`Expected start to return 404, got ${error.response ? error.response.status : error.message}`);
    }
  }

  // Delete fake workspace
  try {
    await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${fakeId}`);
    throw new Error('Expected delete to return 404');
  } catch (error) {
    if (!error.response || error.response.status !== 404) {
      throw new Error(`Expected delete to return 404, got ${error.response ? error.response.status : error.message}`);
    }
  }
});

test('Tier 2.4: Integration Boundaries - Query Non-existent Call Graph method', async () => {
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/call-graph`, {
    params: {
      method: 'com.example.NotExist.foo()',
      direction: 'outgoing'
    }
  });

  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const { edges } = response.data;
  if (!Array.isArray(edges) || edges.length !== 0) {
    throw new Error(`Expected empty edges list, got: ${JSON.stringify(edges)}`);
  }
});

test('Tier 2.5: Integration Boundaries - Query Non-existent Lineage node', async () => {
  const response = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/lineage`, {
    params: {
      node: 'com.example.NotExist.foo#bar',
      direction: 'downstream'
    }
  });

  if (response.status !== 200) {
    throw new Error(`Expected status 200, got ${response.status}`);
  }

  const { nodes, edges } = response.data;
  if (!Array.isArray(edges) || edges.length !== 0) {
    throw new Error(`Expected empty edges list, got: ${JSON.stringify(edges)}`);
  }
});


// ==========================================
// TIER 3: CROSS-FEATURE COMBINATIONS
// ==========================================

test('Tier 3.1: Stopped Workspace + Call Graph Query', async () => {
  // Stop workspace first
  await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/stop`);

  // Attempt to query Call Graph
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/call-graph`, {
      params: {
        method: 'UserController.getUserById',
        direction: 'outgoing'
      }
    });
    throw new Error('Expected query on stopped workspace to fail with 404');
  } catch (error) {
    if (!error.response || error.response.status !== 404) {
      throw new Error(`Expected status 404, got ${error.response ? error.response.status : error.message}`);
    }
  }

  // Restore state for next tests
  await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/start`);
});

test('Tier 3.2: Deleted Workspace + Symbol Resolution', async () => {
  // Create a temporary workspace to delete
  const payload = {
    name: 'temp-workspace-delete',
    project_path: SIMPLE_SPRING_PATH
  };
  const createResponse = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, payload);
  const tempId = createResponse.data.id;

  // Delete it
  await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${tempId}`);

  // Query symbol resolution
  try {
    await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${tempId}/symbol`, {
      params: { fqn: 'com.example.simple.controller.UserController' }
    });
    throw new Error('Expected symbol resolution on deleted workspace to fail');
  } catch (error) {
    if (!error.response || error.response.status !== 404) {
      throw new Error(`Expected 404, got ${error.response ? error.response.status : error.message}`);
    }
  }
});

test('Tier 3.3: Incremental Analysis + Symbol Resolution', async () => {
  // 1. Create a temporary folder copy of simple-spring
  const tempProjectDir = path.join(PROJECT_ROOT, 'target', `temp_incremental_${Date.now()}`);
  fs.mkdirSync(tempProjectDir, { recursive: true });
  
  const copyDir = (src, dest) => {
    fs.mkdirSync(dest, { recursive: true });
    fs.readdirSync(src).forEach(item => {
      const srcPath = path.join(src, item);
      const destPath = path.join(dest, item);
      if (fs.statSync(srcPath).isDirectory()) {
        copyDir(srcPath, destPath);
      } else {
        fs.copyFileSync(srcPath, destPath);
      }
    });
  };
  copyDir(SIMPLE_SPRING_PATH, tempProjectDir);

  // Clean existing db inside temp folder
  const tempDb = path.join(tempProjectDir, '.astro-probe.db');
  if (fs.existsSync(tempDb)) fs.unlinkSync(tempDb);

  try {
    // 2. Initial analysis on copy
    const wsResp1 = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, {
      name: 'temp-incremental',
      project_path: tempProjectDir
    });
    const tempWsId1 = wsResp1.data.id;

    // 3. Modify a source file: add a dummy method to UserController.java
    const userControllerFile = path.join(tempProjectDir, 'src', 'main', 'java', 'com', 'example', 'simple', 'controller', 'UserController.java');
    let fileContent = fs.readFileSync(userControllerFile, 'utf8');
    
    // Inject a new method before the last closing brace
    const insertionPoint = fileContent.lastIndexOf('}');
    const dummyMethod = `\n    @GetMapping("/dummy-test")\n    public ResponseEntity<String> dummyMethod() {\n        return ResponseEntity.ok("dummy");\n    }\n`;
    fileContent = fileContent.substring(0, insertionPoint) + dummyMethod + fileContent.substring(insertionPoint);
    fs.writeFileSync(userControllerFile, fileContent);

    // 4. Trigger analysis again by creating workspace on same path.
    // It should perform incremental file hashing and analyze only the changed file UserController.java.
    const wsResp2 = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, {
      name: 'temp-incremental',
      project_path: tempProjectDir
    });
    const tempWsId2 = wsResp2.data.id;

    // 5. Query symbol resolution for the new dummy method
    const fqn = 'com.example.simple.controller.UserController.dummyMethod()';
    const symResp = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${tempWsId2}/symbol`, {
      params: { fqn }
    });

    if (symResp.status !== 200) {
      throw new Error(`Expected status 200, got ${symResp.status}`);
    }

    const loc = symResp.data;
    if (loc.startLine <= 1) {
      throw new Error(`Symbol resolution returned fallback/invalid line for incremental method: ${JSON.stringify(loc)}`);
    }

    // Verify file content contains the new method on that line
    const currentLines = fs.readFileSync(userControllerFile, 'utf8').split(/\r?\n/);
    const lineContent = currentLines[loc.startLine - 1];
    if (!lineContent.includes('dummyMethod')) {
      throw new Error(`Line ${loc.startLine} does not contain 'dummyMethod': '${lineContent}'`);
    }

    // Clean up workspaces
    await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${tempWsId1}`);
    if (tempWsId1 !== tempWsId2) {
      await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${tempWsId2}`);
    }
  } finally {
    // Clean up filesystem copy
    if (fs.existsSync(tempProjectDir)) {
      fs.rmSync(tempProjectDir, { recursive: true, force: true });
    }
  }
});

test('Tier 3.4: Inactive Workspace Timeout Auto-Resume', async () => {
  // For testing auto-resume, we simulate the timeout by relying on the background thread timeout.
  // The test runner sets ASTRO_PROBE_IDLE_TIMEOUT_SECS=5. So we wait 6 seconds and query.
  console.log('   Waiting 6 seconds for workspace idle timeout...');
  await new Promise(resolve => setTimeout(resolve, 6000));

  // Verify status in list is 'idle'
  const listResp = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces`);
  const wsState = listResp.data.find(w => w.id === activeWorkspaceId);
  console.log(`   Workspace status after wait: ${wsState ? wsState.status : 'not found'}`);
  if (!wsState || wsState.status !== 'idle') {
    throw new Error(`Expected workspace status to transition to 'idle' before auto-resume query, got '${wsState ? wsState.status : 'not found'}'`);
  }

  // Query Call Graph to trigger auto-resume
  const queryResponse = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/call-graph`, {
    params: {
      method: 'UserController.getUserById',
      direction: 'outgoing'
    }
  });

  if (queryResponse.status !== 200) {
    throw new Error(`Expected status 200 from auto-resumed workspace query, got ${queryResponse.status}`);
  }

  // Verify it transitioned back to loaded
  const listRespAfter = await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces`);
  const wsStateAfter = listRespAfter.data.find(w => w.id === activeWorkspaceId);
  if (wsStateAfter.status !== 'loaded') {
    throw new Error(`Expected workspace status to auto-resume to 'loaded', got '${wsStateAfter.status}'`);
  }
});


// ==========================================
// TIER 4: REAL-WORLD APPLICATION SCENARIOS
// ==========================================

test('Tier 4.1: Real-World - Code Navigation Flow', async () => {
  // Scenario: UserController -> UserService -> UserServiceImpl
  
  // 1. Resolve UserController.getUserById FQN
  const loc = (await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`, {
    params: { fqn: 'com.example.simple.controller.UserController.getUserById(java.lang.Long)' }
  })).data;

  // 2. Query call graph outgoing to see what it calls
  const callGraph = (await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/call-graph`, {
    params: {
      method: 'UserController.getUserById',
      direction: 'outgoing'
    }
  })).data;

  // Verify it contains a call edge to UserService.findById
  const edge = callGraph.edges.find(e => e.callee.includes('UserService') && e.callee.includes('findById'));
  if (!edge) {
    throw new Error('Call graph missing connection from UserController to UserService');
  }

  // 3. Resolve the target implementation FQN (UserServiceImpl.findById)
  const implLoc = (await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/symbol`, {
    params: { fqn: 'com.example.simple.service.UserServiceImpl.findById(java.lang.Long)' }
  })).data;

  if (!implLoc.filePath.endsWith('UserServiceImpl.java') || implLoc.startLine <= 1) {
    throw new Error(`Failed to resolve implementation symbol location: ${JSON.stringify(implLoc)}`);
  }
  console.log(`   Navigated to implementation: UserServiceImpl.java at line ${implLoc.startLine}`);
});

test('Tier 4.2: Real-World - Security Data Flow Audit', async () => {
  // Scenario: Trace input userDto from UserController.createUser down to UserRepository.save
  
  const lineage = (await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}/lineage`, {
    params: {
      node: 'UserController.createUser#userDto',
      direction: 'downstream'
    }
  })).data;

  // Verify nodes exist in the flow indicating it goes from controller -> service -> repository
  const nodes = lineage.nodes;
  const hasController = nodes.some(n => n.includes('UserController.createUser') && n.includes('#userDto'));
  const hasService = nodes.some(n => n.includes('UserService') && n.includes('create') && n.includes('#userDto'));
  
  if (!hasController || !hasService) {
    throw new Error(`Data flow audit failed. Expected lineage flow from controller to service. Nodes: ${JSON.stringify(nodes)}`);
  }
  console.log('   Lineage path successfully traced from controller parameters to business services');
});

test('Tier 4.3: Real-World - Spring Event Lineage resolution (complex-spring)', async () => {
  // Scenario: Analyze complex-spring which publishes OrderCreatedEvent and listen to it
  
  // 1. Create workspace for complex-spring
  console.log('   Creating workspace for complex-spring (takes a few seconds for analysis)...');
  const complexResponse = await axios.post(`${MIDDLE_LAYER_URL}/api/workspaces`, {
    name: 'e2e-complex-spring',
    project_path: COMPLEX_SPRING_PATH
  });
  complexWorkspaceId = complexResponse.data.id;
  complexDbPath = complexResponse.data.db_path;

  // 2. Query call graph for EventPublisherService.publishOrderCreated(Object) outgoing
  const callGraph = (await axios.get(`${MIDDLE_LAYER_URL}/api/workspaces/${complexWorkspaceId}/call-graph`, {
    params: {
      method: 'publishOrderCreated',
      direction: 'outgoing'
    }
  })).data;

  // Verify we can find a virtual edge from event publishing to the listener:
  // e.g., NotificationEventListener.onOrderCreated
  const hasEventEdge = callGraph.edges.some(e => 
    e.caller.includes('EventPublisherService') && 
    e.callee.includes('NotificationEventListener')
  );

  if (!hasEventEdge) {
    throw new Error(`Expected Spring event dispatcher edge to NotificationEventListener, edges: ${JSON.stringify(callGraph.edges)}`);
  }
  console.log('   Successfully resolved Spring Event propagation edge in Call Graph');
});

test('Tier 4.4: Real-World - Clean Up Workspaces and Temp Files', async () => {
  // Clean up simple-spring workspace
  if (activeWorkspaceId) {
    const deleteResp = await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${activeWorkspaceId}`);
    if (deleteResp.status !== 200 || !deleteResp.data.success) {
      throw new Error(`Failed to delete simple-spring workspace: ${JSON.stringify(deleteResp.data)}`);
    }
    if (activeDbPath && fs.existsSync(activeDbPath)) {
      throw new Error('Database file was not deleted from dynamic data path');
    }
  }

  // Clean up complex-spring workspace
  if (complexWorkspaceId) {
    const deleteResp = await axios.delete(`${MIDDLE_LAYER_URL}/api/workspaces/${complexWorkspaceId}`);
    if (deleteResp.status !== 200 || !deleteResp.data.success) {
      throw new Error(`Failed to delete complex-spring workspace: ${JSON.stringify(deleteResp.data)}`);
    }
    if (complexDbPath && fs.existsSync(complexDbPath)) {
      throw new Error('Database file was not deleted from dynamic data path');
    }
  }
  
  console.log('   All workspaces deleted successfully, SQLite DB files wiped');
});

// Run all defined tests
async function runTests() {
  console.log(`Running E2E tests against ${MIDDLE_LAYER_URL}...\n`);
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
  console.log(`E2E TEST RUN SUMMARY:`);
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

// Execute tests if run directly
if (require.main === module) {
  runTests();
}

module.exports = { runTests };
