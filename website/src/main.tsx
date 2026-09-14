import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import App from "./App";
import { UpdatesPage } from "./components/updates-page";
import "./index.css";

const container = document.getElementById("root")!;
// Both HTML entries share this bundle; the page is chosen by the marker the
// prerender leaves on the root (see scripts/prerender.mjs).
const Page = container.dataset.page === "updates" ? UpdatesPage : App;
const tree = (
  <StrictMode>
    <Page />
  </StrictMode>
);

if (container.hasChildNodes()) {
  // The build prerendered the app markup (scripts/prerender.mjs).
  hydrateRoot(container, tree);
} else {
  createRoot(container).render(tree);
}
