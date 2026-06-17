const fs = require('fs');
const path = require('path');

function copyDir(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  const entries = fs.readdirSync(src, { withFileTypes: true });

  for (const entry of entries) {
    const srcPath = path.join(src, entry.name);
    const destPath = path.join(dest, entry.name);

    if (entry.isDirectory()) {
      copyDir(srcPath, destPath);
    } else {
      fs.copyFileSync(srcPath, destPath);
    }
  }
}

try {
  const monacoSrc = path.resolve(__dirname, 'node_modules/monaco-editor/min/vs');
  const monacoDest = path.resolve(__dirname, 'dist/monaco/vs');
  
  if (fs.existsSync(monacoSrc)) {
    console.log(`Copying Monaco assets from ${monacoSrc} to ${monacoDest}...`);
    copyDir(monacoSrc, monacoDest);
    console.log('Monaco assets copied successfully.');
  } else {
    console.warn(`Warning: Monaco source assets not found at ${monacoSrc}. Skipping copy.`);
  }
} catch (err) {
  console.error('Failed to copy Monaco assets:', err.message);
}
