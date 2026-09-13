import { StrictMode } from "react";
import { renderToString } from "react-dom/server";
import App from "./App";

// The build prerenders the app into dist/index.html (see scripts/prerender.mjs)
// so crawlers — and most AI crawlers, which skip JavaScript — see the full
// page. The client bundle hydrates this markup on load.
export function render(): string {
  return renderToString(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
