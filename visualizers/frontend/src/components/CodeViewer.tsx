import { useEffect, useRef, useState } from 'react';
import Editor, { loader } from '@monaco-editor/react';
import { ExternalLink } from 'lucide-react';

const isPositiveInteger = (val: any): boolean => {
  const n = Number(val);
  return !isNaN(n) && Number.isInteger(n) && n >= 1;
};

// Configure Monaco loader to use local assets to bypass external CDN fallback
loader.config({ paths: { vs: '/monaco/vs' } });

interface Coordinate {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
}

interface CodeViewerProps {
  filePath: string | null;
  fileContent: string;
  symbolCoordinate: Coordinate | null;
  projectPath: string | null;
}

export function CodeViewer({ filePath, fileContent, symbolCoordinate, projectPath }: CodeViewerProps) {
  const [isEditorReady, setIsEditorReady] = useState(false);
  const editorRef = useRef<any>(null);
  const monacoRef = useRef<any>(null);
  const decorationsRef = useRef<string[]>([]);

  const handleEditorDidMount = (editor: any, monaco: any) => {
    editorRef.current = editor;
    monacoRef.current = monaco;
    setIsEditorReady(true);
  };

  useEffect(() => {
    if (!editorRef.current || !monacoRef.current || !symbolCoordinate) return;

    const editor = editorRef.current;
    const monaco = monacoRef.current;
    const { startLine, startColumn, endLine, endColumn } = symbolCoordinate;

    const startL = Number(startLine);
    const startC = Number(startColumn);
    const endL = Number(endLine);
    const endC = Number(endColumn);

    if (!isPositiveInteger(startL) || !isPositiveInteger(startC) || !isPositiveInteger(endL) || !isPositiveInteger(endC)) {
      console.warn('Invalid or non-positive coordinates found. Skipping highlight:', symbolCoordinate);
      return;
    }

    // Center the viewport on the start line
    editor.revealLineInCenter(startL);

    // Create custom decoration for highlight
    const range = new monaco.Range(startL, startC, endL, endC);
    const newDecorations = [
      {
        range,
        options: {
          isWholeLine: false,
          inlineClassName: 'bg-amber-500/30 border-b border-amber-500 font-semibold text-white',
          glyphMarginClassName: 'bg-amber-500 rounded-full w-2.5 h-2.5 ml-1',
          hoverMessage: { value: '🎯 **Astro-Probe Analyzed Symbol**' }
        }
      }
    ];

    decorationsRef.current = editor.deltaDecorations(decorationsRef.current, newDecorations);
  }, [symbolCoordinate, fileContent, isEditorReady]);

  // Clean up decorations if file path or coordinate becomes null
  useEffect(() => {
    if (!symbolCoordinate && editorRef.current && decorationsRef.current.length > 0) {
      editorRef.current.deltaDecorations(decorationsRef.current, []);
      decorationsRef.current = [];
    }
  }, [filePath, symbolCoordinate]);

  const getRelativePath = (proj: string, file: string) => {
    const pPath = proj.replace(/\\/g, '/').replace(/\/$/, '');
    const fPath = file.replace(/\\/g, '/');
    if (fPath.toLowerCase().startsWith(pPath.toLowerCase())) {
      if (fPath.length === pPath.length || fPath.charAt(pPath.length) === '/') {
        return fPath.substring(pPath.length).replace(/^\//, '');
      }
    }
    return fPath;
  };

  const handleOpenInZed = () => {
    if (!filePath) return;
    let line = 1;
    let col = 1;
    if (symbolCoordinate && isPositiveInteger(symbolCoordinate.startLine) && isPositiveInteger(symbolCoordinate.startColumn)) {
      line = Number(symbolCoordinate.startLine);
      col = Number(symbolCoordinate.startColumn);
    }
    const normalizedPath = filePath.replace(/\\/g, '/');
    const encodedPath = encodeURIComponent(normalizedPath).replace(/%2F/g, '/').replace(/%3A/g, ':');
    const zedUri = `zed://file/${encodedPath}:${line}:${col}`;
    window.location.href = zedUri;
  };

  return (
    <div className="w-full h-full min-h-0 border border-zinc-700 rounded-lg flex flex-col bg-zinc-950 overflow-hidden">
      {/* Code Header / Status bar */}
      <div className="bg-zinc-900 border-b border-zinc-800 px-4 py-2 text-xs font-mono text-slate-400 truncate flex items-center justify-between">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-amber-500 font-semibold">JAVA</span>
          <span className="truncate" title={filePath || ''}>
            {filePath ? getRelativePath(projectPath || '', filePath) : 'No file loaded'}
          </span>
        </div>
        <div className="flex items-center gap-4 shrink-0">
          {symbolCoordinate && isPositiveInteger(symbolCoordinate.startLine) && isPositiveInteger(symbolCoordinate.startColumn) && (
            <span className="text-zinc-500">
              Line {symbolCoordinate.startLine}:{symbolCoordinate.startColumn}
            </span>
          )}
          {filePath && (
            <button
              onClick={handleOpenInZed}
              className="flex items-center gap-1 bg-zinc-800 hover:bg-zinc-700 text-slate-200 px-2 py-0.5 rounded border border-zinc-700 transition"
              title="Open in local Zed editor"
            >
              <ExternalLink size={12} />
              <span>Open in Zed</span>
            </button>
          )}
        </div>
      </div>

      {/* Monaco Container */}
      <div className="flex-1 min-h-0 relative bg-zinc-950">
        <Editor
          height="100%"
          language="java"
          theme="vs-dark"
          value={fileContent}
          onMount={handleEditorDidMount}
          options={{
            readOnly: true,
            minimap: { enabled: true },
            lineNumbers: 'on',
            fontSize: 13,
            automaticLayout: true,
            glyphMargin: true,
            scrollBeyondLastLine: false,
            folding: true,
            lineDecorationsWidth: 10
          }}
        />
      </div>
    </div>
  );
}
