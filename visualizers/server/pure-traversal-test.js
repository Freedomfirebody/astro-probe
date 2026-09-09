const fs = require('fs');
const path = require('path');

const projectPath = path.resolve(__dirname, '../../test-samples/simple-spring');

function testPathTraversal(filePath) {
  try {
    // Resolve projectPath to canonical/absolute path
    let absoluteProjectPath;
    try {
      absoluteProjectPath = fs.realpathSync(projectPath);
    } catch (err) {
      return { status: 404, error: `Workspace project path not found on disk` };
    }

    // Resolve target path (it can be absolute or relative to projectPath)
    const resolvedPath = path.isAbsolute(filePath)
      ? filePath
      : path.resolve(absoluteProjectPath, filePath);

    // 1. Initial Path Traversal Check (Catching traversals even if file does not exist)
    const relativePre = path.relative(absoluteProjectPath, resolvedPath);
    if (relativePre === '' || relativePre.startsWith('..') || path.isAbsolute(relativePre)) {
      return { status: 403, error: 'Access denied: Path traversal detected' };
    }

    // Get realpath of target file (requires file to exist)
    let absoluteFilePath;
    try {
      absoluteFilePath = fs.realpathSync(resolvedPath);
    } catch (err) {
      return { status: 404, error: `File not found: ${filePath}` };
    }

    // 2. Canonical Path Traversal Check (Handling symbolic links pointing outside)
    const relativePost = path.relative(absoluteProjectPath, absoluteFilePath);
    if (relativePost === '' || relativePost.startsWith('..') || path.isAbsolute(relativePost)) {
      return { status: 403, error: 'Access denied: Path traversal detected' };
    }

    return { status: 200, content: 'File content placeholder' };
  } catch (error) {
    return { status: 500, error: error.message };
  }
}

const testCases = [
  { filePath: '../../package.json', expectedStatus: 403, desc: 'Traversal to existing file outside workspace' },
  { filePath: '../non-existent', expectedStatus: 403, desc: 'Traversal to non-existent file outside workspace' },
  { filePath: 'src/main/java/com/example/simple/controller/UserController.java', expectedStatus: 200, desc: 'Valid existing file inside workspace' },
  { filePath: 'src/main/java/NonExistent.java', expectedStatus: 404, desc: 'Non-existent file inside workspace' },
  { filePath: 'C:\\Windows\\win.ini', expectedStatus: 403, desc: 'Absolute path on another drive/outside workspace' },
  { filePath: '.', expectedStatus: 403, desc: 'Workspace directory itself (relative .)' },
  { filePath: './', expectedStatus: 403, desc: 'Workspace directory itself (relative ./)' },
  { filePath: 'src/main/java/com/example/simple/controller/../controller/UserController.java', expectedStatus: 200, desc: 'Valid file with internal traversal resolution' }
];

let failed = 0;
console.log('=== RUNNING PURE PATH TRAVERSAL UNIT TESTS ===\n');

testCases.forEach((tc, idx) => {
  const res = testPathTraversal(tc.filePath);
  const pass = res.status === tc.expectedStatus;
  console.log(`[${idx + 1}] ${pass ? 'PASS' : 'FAIL'} - ${tc.desc}`);
  console.log(`    filePath:       ${tc.filePath}`);
  console.log(`    expectedStatus: ${tc.expectedStatus}`);
  console.log(`    actualStatus:   ${res.status}`);
  if (res.error) {
    console.log(`    error:          ${res.error}`);
  }
  if (!pass) {
    failed++;
  }
  console.log('');
});

console.log(`=== SUMMARY: ${failed === 0 ? 'ALL PASSED' : `${failed} FAILED`} ===`);
process.exit(failed === 0 ? 0 : 1);
