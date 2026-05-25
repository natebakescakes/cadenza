import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { AppShell } from "@/components/AppShell";
import { DbGate } from "@/components/DbGate";
import { Toaster } from "@/components/ui/sonner";
import { UpdateChecker } from "@/components/UpdateChecker";
import Dashboard from "@/pages/Dashboard";
import Analytics from "@/pages/Wpm";
import Suggestions from "@/pages/Suggestions";
import Proficiency from "@/pages/Proficiency";
import Device from "@/pages/Device";
import Settings from "@/pages/Settings";

function App() {
  return (
    <DbGate>
      <HashRouter>
        <Routes>
          <Route element={<AppShell />}>
            <Route index element={<Dashboard />} />
            <Route path="analytics" element={<Analytics />} />
            <Route path="wpm" element={<Navigate to="/analytics" replace />} />
            <Route path="suggestions" element={<Suggestions />} />
            <Route path="proficiency" element={<Proficiency />} />
            <Route path="device" element={<Device />} />
            <Route path="settings" element={<Settings />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </HashRouter>
      <Toaster position="bottom-right" richColors />
      <UpdateChecker />
    </DbGate>
  );
}

export default App;
