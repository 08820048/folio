import { StrictMode } from "react";
import { renderToString } from "react-dom/server";
import App from "./App";
import { UpdatesPage } from "./components/updates-page";

// The prerender script reads this to fill `softwareVersion` in the structured
// data, so the page and the JSON-LD share one release history.
export { latestRelease } from "./data/releases";

// The build prerenders each page into its HTML file (see
// scripts/prerender.mjs) so crawlers — and most AI crawlers, which skip
// JavaScript — see the full page. The client bundle hydrates this markup.

export function render(): string {
  return renderToString(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

export function renderUpdates(): string {
  return renderToString(
    <StrictMode>
      <UpdatesPage />
    </StrictMode>,
  );
}
