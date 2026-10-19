import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ApprovalView } from "./views/ApprovalView";
import { isTauri, setApi } from "./ipc/api";
import "./index.css";

async function start() {
  if (isTauri()) {
    setApi((await import("./ipc/tauri")).tauriApi);
  } else if (import.meta.env.DEV) {
    // Plain-browser preview: a simulated backend, never shipped in release builds.
    setApi((await import("./ipc/mock")).createMockApi());
  } else {
    throw new Error("Boxdeck must run inside its desktop shell");
  }
  // Approval prompts load the same bundle in their own small window.
  const approval = new URLSearchParams(window.location.search).get("approval");
  const queryClient = new QueryClient({
    defaultOptions: { queries: { staleTime: 5_000, refetchOnWindowFocus: false } },
  });
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        {approval ? <ApprovalView id={approval} /> : <App />}
      </QueryClientProvider>
    </React.StrictMode>,
  );
}

void start();
