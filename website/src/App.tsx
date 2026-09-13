import { EditorDemo } from "@/components/editor-demo";
import { Hero } from "@/components/hero";
import {
  Details,
  Faq,
  Features,
  FinalCta,
  Principles,
  SiteFooter,
  Workflow,
} from "@/components/sections";
import { SiteHeader } from "@/components/site-header";
import { ThemeProvider } from "@/components/theme";

export default function App() {
  return (
    <ThemeProvider>
      <a
        href="#main"
        className="fixed top-3 left-4 z-50 -translate-y-20 rounded-md bg-brand px-4 py-2.5 text-sm text-primary-foreground transition-transform focus-visible:translate-y-0"
      >
        Skip to content
      </a>
      <SiteHeader />
      <main id="main">
        <Hero />
        <EditorDemo />
        <div className="pt-10">
          <Principles />
        </div>
        <Features />
        <Workflow />
        <Details />
        <Faq />
        <FinalCta />
      </main>
      <SiteFooter />
    </ThemeProvider>
  );
}
