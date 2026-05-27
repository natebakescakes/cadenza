import React from "react";
import ReactDOM from "react-dom/client";
import OverlayRoot from "./pages/OverlayRoot";
import "./styles.css";

// The overlay reuses Cadenza's dark theme tokens; apply the theme class.
// Transparency is enforced by overlay.html's inline style (overrides the
// opaque base-layer body background) so the NSPanel shows through.
document.documentElement.classList.add("dark");

ReactDOM.createRoot(document.getElementById("overlay-root") as HTMLElement).render(
  <React.StrictMode>
    <OverlayRoot />
  </React.StrictMode>,
);
