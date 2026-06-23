const fs = require('fs');
const path = require('path');
const assert = require('assert');
const axios = require('axios');
const Database = require('better-sqlite3');

const { resolveJavaSymbol, findFileByFqn } = require('./src/services/symbolResolver');
const app = require('./src/app');

const projectPath = path.resolve(__dirname, '../../test-samples/simple-spring');

async function runTests() {
  console.log('=== RUNNING CHALLENGER SECURITY & ROBUSTNESS TESTS ===\n');

  let failedTests = 0;

  // ----------------------------------------------------
  // TEST 1: Path Traversal Rejected via FQN Segments
  // ----------------------------------------------------
  console.log('--- TEST 1: Path Traversal in FQN Segments ---');
  const maliciousFqns = [
    'com.example.simple.controller.UserController/../../../../etc/passwd',
    '..\\..\\',
    'com.example.simple.controller.UserController/../../',
    'com.example.simple.controller.UserController/..\\..\\',
    'com.example.simple.controller.UserController\\..\\..\\',
    '../../etc/passwd',
    '..\\..\\windows\\win.ini'
  ];

  for (const fqn of maliciousFqns) {
    try {
      resolveJavaSymbol(projectPath, fqn);
      console.error(`[FAIL] FQN "${fqn}" was not rejected!`);
      failedTests++;
    } catch (err) {
      if (err.message.includes('Invalid symbol FQN')) {
        console.log(`[PASS] FQN "${fqn}" correctly rejected: ${err.message}`);
      } else {
        console.error(`[FAIL] FQN "${fqn}" rejected with unexpected error: ${err.message}`);
        failedTests++;
      }
    }
  }

  // ----------------------------------------------------
  // TEST 2: Path Traversal Mitigation for DB File Paths
  // ----------------------------------------------------
  console.log('\n--- TEST 2: Path Traversal via SQLite DB Spoofing ---');
  const tempProjectDir = path.resolve(__dirname, 'temp_test_project');
  const tempDbPath = path.join(tempProjectDir, '.astro-probe.db');
  const maliciousPath = path.resolve(__dirname, 'outside_file.java');

  try {
    if (!fs.existsSync(tempProjectDir)) fs.mkdirSync(tempProjectDir);
    
    // Write a file outside the project directory
    fs.writeFileSync(maliciousPath, 'public class TraveralClass {}');

    // Create a mock SQLite db inside the project directory
    if (fs.existsSync(tempDbPath)) fs.unlinkSync(tempDbPath);
    const db = new Database(tempDbPath);
    db.exec(`
      CREATE TABLE IF NOT EXISTS file_facts_metadata (
        class_fqn TEXT PRIMARY KEY,
        file_path TEXT
      );
    `);

    // Insert class mapping pointing outside the project path
    const outerClassFqn = 'com.example.TraversalClass';
    db.prepare('INSERT INTO file_facts_metadata (class_fqn, file_path) VALUES (?, ?)')
      .run(outerClassFqn, maliciousPath);
    db.close();

    // Now call resolveJavaSymbol on the spoofed class FQN
    try {
      resolveJavaSymbol(tempProjectDir, outerClassFqn);
      console.error('[FAIL] Class pointing to outside file was resolved without error!');
      failedTests++;
    } catch (err) {
      if (err.message.includes('Access denied: Path traversal detected')) {
        console.log(`[PASS] Path traversal attack via DB spoofing detected and rejected: ${err.message}`);
      } else {
        console.error(`[FAIL] Traversal attack rejected with unexpected error: ${err.message}`);
        failedTests++;
      }
    }
  } catch (err) {
    console.error(`[ERROR] Test 2 setup/teardown failed: ${err.message}`);
    failedTests++;
  } finally {
    // Clean up
    if (fs.existsSync(tempDbPath)) fs.unlinkSync(tempDbPath);
    if (fs.existsSync(maliciousPath)) fs.unlinkSync(maliciousPath);
    if (fs.existsSync(tempProjectDir)) fs.rmdirSync(tempProjectDir);
  }

  // ----------------------------------------------------
  // TEST 3: Cyclic Symbolic Link Robustness
  // ----------------------------------------------------
  console.log('\n--- TEST 3: Cyclic Symbolic Link Robustness ---');
  const cyclicProjectDir = path.resolve(__dirname, 'temp_cyclic_project');
  const subdir = path.join(cyclicProjectDir, 'subdir');
  const cycleLink = path.join(subdir, 'cycle');

  try {
    if (fs.existsSync(cycleLink)) {
      try { fs.unlinkSync(cycleLink); } catch (e) { fs.rmdirSync(cycleLink); }
    }
    if (fs.existsSync(subdir)) fs.rmdirSync(subdir);
    if (fs.existsSync(cyclicProjectDir)) fs.rmdirSync(cyclicProjectDir);

    fs.mkdirSync(cyclicProjectDir);
    fs.mkdirSync(subdir);

    // Create a junction loop: cycle -> cyclicProjectDir
    fs.symlinkSync(cyclicProjectDir, cycleLink, 'junction');

    console.log('Walking directory with cyclic symlink...');
    const walkStart = Date.now();
    const result = findFileByFqn(cyclicProjectDir, 'com.example.NonExistentClass');
    const walkDuration = Date.now() - walkStart;

    console.log(`[INFO] Cyclic file walk completed in ${walkDuration}ms`);
    assert.strictEqual(result, null, 'Expected no file found for non-existent class');
    console.log('[PASS] File walking terminated safely without infinite loops or stack overflow.');
  } catch (err) {
    console.error(`[FAIL] Cyclic link test failed: ${err.message}`);
    failedTests++;
  } finally {
    // Clean up
    if (fs.existsSync(cycleLink)) fs.rmdirSync(cycleLink);
    if (fs.existsSync(subdir)) fs.rmdirSync(subdir);
    if (fs.existsSync(cyclicProjectDir)) fs.rmdirSync(cyclicProjectDir);
  }

  // ----------------------------------------------------
  // TEST 4: API Endpoint Rejection (HTTP Server)
  // ----------------------------------------------------
  console.log('\n--- TEST 4: HTTP API Rejection ---');
  const symbolResolver = require('./src/services/symbolResolver');
  const originalGetWorkspacePath = symbolResolver.getWorkspacePath;

  // Mock getWorkspacePath to return our test project path
  symbolResolver.getWorkspacePath = async (workspaceId) => {
    return projectPath;
  };

  const server = app.listen(0, async () => {
    const port = server.address().port;
    const client = axios.create({
      baseURL: `http://localhost:${port}`,
      validateStatus: () => true // Do not throw on 4xx/5xx status
    });

    try {
      for (const fqn of maliciousFqns) {
        const res = await client.get(`/api/workspaces/1/symbol`, { params: { fqn } });
        if (res.status === 400 && res.data.error === 'Invalid symbol FQN') {
          console.log(`[PASS] API endpoint rejected FQN "${fqn}" with HTTP 400: ${JSON.stringify(res.data)}`);
        } else {
          console.error(`[FAIL] API endpoint did not reject FQN "${fqn}" correctly. Status: ${res.status}, Body: ${JSON.stringify(res.data)}`);
          failedTests++;
        }
      }
    } catch (err) {
      console.error(`[ERROR] API request failed: ${err.message}`);
      failedTests++;
    } finally {
      server.close();
      symbolResolver.getWorkspacePath = originalGetWorkspacePath;
    }
  });

  // Wait for the HTTP server test to finish before running performance tests
  await new Promise(resolve => server.on('close', resolve));

  // ----------------------------------------------------
  // TEST 4.5: File Endpoint Path Traversal & Content Retrieval
  // ----------------------------------------------------
  console.log('\n--- TEST 4.5: File Endpoint Security & Retrieval ---');
  const fileServer = app.listen(0, async () => {
    const port = fileServer.address().port;
    const client = axios.create({
      baseURL: `http://localhost:${port}`,
      validateStatus: () => true
    });

    try {
      symbolResolver.getWorkspacePath = async (workspaceId) => {
        return projectPath;
      };

      // 1. Check successful retrieval of UserController.java
      const targetRelPath = 'src/main/java/com/example/simple/controller/UserController.java';
      const resOk = await client.get(`/api/workspaces/1/file`, {
        params: { filePath: targetRelPath }
      });
      if (resOk.status === 200 && resOk.data.content && resOk.data.content.includes('UserController')) {
        console.log(`[PASS] Correctly retrieved file content for ${targetRelPath}`);
      } else {
        console.error(`[FAIL] File retrieval failed for ${targetRelPath}. Status: ${resOk.status}, Body: ${JSON.stringify(resOk.data)}`);
        failedTests++;
      }

      // 2. Check path traversal rejection with 403/404
      const traversalPaths = [
        '../../package.json',
        '../non-existent',
        '/etc/passwd',
        '..\\..\\windows\\win.ini'
      ];
      for (const badPath of traversalPaths) {
        const resTraversal = await client.get(`/api/workspaces/1/file`, {
          params: { filePath: badPath }
        });
        if (resTraversal.status === 403 || resTraversal.status === 404) {
          if (badPath.includes('package.json')) {
            if (resTraversal.status === 403) {
              console.log(`[PASS] Correctly rejected path traversal for ${badPath} with 403 Forbidden.`);
            } else {
              console.error(`[FAIL] Expected 403 Forbidden for existing traversal file ${badPath}, got status: ${resTraversal.status}`);
              failedTests++;
            }
          } else {
            console.log(`[PASS] Rejected traversal or missing file ${badPath} with status ${resTraversal.status}`);
          }
        } else {
          console.error(`[FAIL] Traversal path ${badPath} was not rejected! Status: ${resTraversal.status}, Body: ${JSON.stringify(resTraversal.data)}`);
          failedTests++;
        }
      }

      // 3. Check missing filePath returns 400 Bad Request
      const resMissing = await client.get(`/api/workspaces/1/file`);
      if (resMissing.status === 400) {
        console.log('[PASS] Missing filePath parameter returned 400 Bad Request.');
      } else {
        console.error(`[FAIL] Expected 400 for missing filePath, got status: ${resMissing.status}`);
        failedTests++;
      }

    } catch (err) {
      console.error(`[ERROR] File Endpoint test failed: ${err.message}`);
      failedTests++;
    } finally {
      fileServer.close();
      symbolResolver.getWorkspacePath = originalGetWorkspacePath;
    }
  });

  await new Promise(resolve => fileServer.on('close', resolve));

  // ----------------------------------------------------
  // TEST 5: Performance Verification
  // ----------------------------------------------------
  console.log('\n--- TEST 5: Performance Verification ---');
  const testFqn = 'com.example.simple.controller.UserController.getUserById(java.lang.Long)#id';

  try {
    const runs = 100;
    const times = [];

    // Run once to warm up (loads java-parser, caches class files, etc.)
    const warmupStart = Date.now();
    const warmupResult = resolveJavaSymbol(projectPath, testFqn);
    const warmupTime = Date.now() - warmupStart;
    console.log(`[INFO] Cold Start (Warmup Run) time: ${warmupTime}ms`);
    assert.ok(warmupResult.filePath, 'Valid FQN should resolve');

    for (let i = 0; i < runs; i++) {
      const start = Date.now();
      resolveJavaSymbol(projectPath, testFqn);
      times.push(Date.now() - start);
    }

    const min = Math.min(...times);
    const max = Math.max(...times);
    const avg = times.reduce((a, b) => a + b, 0) / times.length;

    console.log(`[INFO] Resolution Performance over ${runs} runs:`);
    console.log(`       Min: ${min}ms`);
    console.log(`       Max: ${max}ms`);
    console.log(`       Avg (Warm): ${avg.toFixed(2)}ms`);

    if (avg < 50) {
      console.log(`[PASS] FQN Resolution warm average is ${avg.toFixed(2)}ms (<50ms limit).`);
    } else {
      console.error(`[FAIL] FQN Resolution warm average is ${avg.toFixed(2)}ms, which exceeds the 50ms limit.`);
      failedTests++;
    }

    if (warmupTime < 1000) {
      console.log(`[PASS] Cold start time is sub-second (${warmupTime}ms).`);
    } else {
      console.warn(`[WARN] Cold start time is above 1 second (${warmupTime}ms).`);
    }
  } catch (err) {
    console.error(`[FAIL] Performance test failed with error: ${err.message}`);
    failedTests++;
  }

  // ----------------------------------------------------
  // TEST 6: Generic Bracket FQN Parsing and Resolution
  // ----------------------------------------------------
  console.log('\n--- TEST 6: Generic Bracket FQN Parsing and Resolution ---');
  try {
    const genericFqns = [
      'com.example.simple.controller.UserController<java.lang.String>.getUserById(java.lang.Long)',
      'com.example.simple.controller.UserController<com.example.simple.dto.UserDto>.getUserById(java.lang.Long)#id',
      'com.example.simple.controller.UserController<T>'
    ];

    for (const fqn of genericFqns) {
      const result = resolveJavaSymbol(projectPath, fqn);
      assert.ok(result.filePath, `FQN "${fqn}" should resolve`);
      assert.ok(result.startLine > 0, `FQN "${fqn}" should return valid startLine`);
      console.log(`[PASS] FQN "${fqn}" resolved successfully to: ${result.filePath}:${result.startLine}`);
    }
  } catch (err) {
    console.error(`[FAIL] Generic bracket test failed: ${err.message}`);
    failedTests++;
  }

  console.log('\n=== CHALLENGER SUMMARY ===');
  if (failedTests === 0) {
    console.log('ALL TESTS PASSED SUCCESSFULLY! The Middle-Layer server is secure and robust.');
    process.exit(0);
  } else {
    console.error(`${failedTests} TEST(S) FAILED. Please check logs.`);
    process.exit(1);
  }
}

runTests().catch(err => {
  console.error('Fatal test runner error:', err);
  process.exit(1);
});
