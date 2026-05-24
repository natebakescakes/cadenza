import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// Cadenza is a dark-only experience; apply the theme class up front.
document.documentElement.classList.add("dark");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
