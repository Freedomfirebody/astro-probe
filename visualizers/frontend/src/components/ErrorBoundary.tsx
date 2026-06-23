import { Component, ErrorInfo, ReactNode } from 'react';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error("ErrorBoundary caught an error:", error, errorInfo);
  }

  public render() {
    if (this.state.hasError) {
      if (this.props.fallback) {
        return this.props.fallback;
      }
      return (
        <div className="p-6 border border-rose-500/30 bg-rose-500/10 rounded-lg text-rose-200 flex flex-col gap-2 my-2">
          <div className="flex items-center gap-2">
            <span className="w-2.5 h-2.5 bg-rose-500 rounded-full animate-pulse"></span>
            <h3 className="font-semibold text-rose-400">Visualization Component Error</h3>
          </div>
          <p className="text-sm text-zinc-300">
            An exception occurred while rendering this component. The visualizer state has been isolated.
          </p>
          <pre className="text-xs font-mono bg-zinc-900/60 p-3 rounded border border-zinc-800 overflow-x-auto text-zinc-400 mt-2">
            {this.state.error?.message || String(this.state.error)}
          </pre>
          <button
            onClick={() => this.setState({ hasError: false, error: null })}
            className="self-start mt-2 px-3 py-1 bg-zinc-800 hover:bg-zinc-700 text-zinc-200 text-xs rounded border border-zinc-700 transition"
          >
            Retry Render
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
