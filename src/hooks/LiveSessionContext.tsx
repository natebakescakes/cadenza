import { createContext, useContext, type ReactNode } from "react";
import { useLiveSession, type LiveSession } from "./useLiveSession";

const LiveSessionContext = createContext<LiveSession | null>(null);

export function LiveSessionProvider({ children }: { children: ReactNode }) {
  const session = useLiveSession();
  return (
    <LiveSessionContext.Provider value={session}>
      {children}
    </LiveSessionContext.Provider>
  );
}

export function useLiveSessionContext(): LiveSession {
  const ctx = useContext(LiveSessionContext);
  if (!ctx) throw new Error("useLiveSessionContext must be used within LiveSessionProvider");
  return ctx;
}
