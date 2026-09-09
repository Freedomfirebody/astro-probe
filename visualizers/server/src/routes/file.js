const express = require('express');
const router = express.Router({ mergeParams: true });
const fs = require('fs');
const path = require('path');
const { getWorkspacePath } = require('../services/symbolResolver');

// List of allowed file extensions
const ALLOWED_EXTENSIONS = ['.java', '.xml', '.properties', '.yaml', '.yml', '.json', '.txt'];

router.get('/', async (req, res) => {
  const { id } = req.params;
  const { filePath } = req.query;

  if (typeof filePath !== 'string') {
    return res.status(400).json({ error: 'Parameter filePath must be a string' });
  }

  if (!filePath) {
    return res.status(400).json({ error: 'Missing parameter filePath' });
  }

  try {
    const workspaceInfo = await getWorkspacePath(id);
    if (!workspaceInfo) {
      return res.status(404).json({ error: `Workspace with ID ${id} not found on backend` });
    }
    const { projectPath } = workspaceInfo;

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
    const isCaseInsensitive = process.platform === 'win32' || process.platform === 'darwin';
    const checkProjectPath = isCaseInsensitive ? absoluteProjectPath.toLowerCase() : absoluteProjectPath;
    const checkResolvedPath = isCaseInsensitive ? resolvedPath.toLowerCase() : resolvedPath;

    const relativePre = path.relative(checkProjectPath, checkResolvedPath);
    const isTraversalPre = relativePre === '' || 
      path.isAbsolute(relativePre) || 
      relativePre.split(/[/\\]/).includes('..');

    if (isTraversalPre) {
      return res.status(403).json({ error: 'Access denied: Path traversal detected' });
    }

    // Block hidden files/directories in relative path
    const segmentsPre = relativePre.split(/[/\\]/);
    if (segmentsPre.some(seg => seg.startsWith('.') && seg !== '.' && seg !== '..')) {
      return res.status(403).json({ error: 'Access denied: Hidden files/directories are blocked' });
    }

    // Restrict file extension check on resolvedPath pre-check to prevent traversal probes on unauthorized types
    const extPre = path.extname(resolvedPath).toLowerCase();
    if (extPre === '') {
      const filenamePre = path.basename(resolvedPath).toLowerCase();
      const whitelist = ['license', 'readme', 'dockerfile', 'makefile'];
      if (!whitelist.includes(filenamePre)) {
        return res.status(403).json({ error: 'Access denied: Empty file extension is not allowed' });
      }
    } else if (!ALLOWED_EXTENSIONS.includes(extPre)) {
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
    const checkAbsoluteFilePath = isCaseInsensitive ? absoluteFilePath.toLowerCase() : absoluteFilePath;
    const relativePost = path.relative(checkProjectPath, checkAbsoluteFilePath);
    const isTraversalPost = relativePost === '' || 
      path.isAbsolute(relativePost) || 
      relativePost.split(/[/\\]/).includes('..');

    if (isTraversalPost) {
      return res.status(403).json({ error: 'Access denied: Path traversal detected' });
    }

    // Block hidden files/directories in post-realpath path
    const segmentsPost = relativePost.split(/[/\\]/);
    if (segmentsPost.some(seg => seg.startsWith('.') && seg !== '.' && seg !== '..')) {
      return res.status(403).json({ error: 'Access denied: Hidden files/directories are blocked' });
    }

    // Double-check extension on resolved absolute path (following symlinks)
    const extPost = path.extname(absoluteFilePath).toLowerCase();
    if (extPost === '') {
      const filenamePost = path.basename(absoluteFilePath).toLowerCase();
      const whitelist = ['license', 'readme', 'dockerfile', 'makefile'];
      if (!whitelist.includes(filenamePost)) {
        return res.status(403).json({ error: 'Access denied: Empty file extension is not allowed' });
      }
    } else if (!ALLOWED_EXTENSIONS.includes(extPost)) {
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

module.exports = router;
