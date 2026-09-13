import { ArrowUpRight, Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTheme } from "@/components/theme";

const links = [
  { href: "#features", label: "Features" },
  { href: "#workflow", label: "Workflow" },
  { href: "#faq", label: "FAQ" },
];

export function SiteHeader() {
  const { dark, toggle } = useTheme();

  return (
    <header className="border-b">
      <div className="mx-auto flex h-16 w-full max-w-6xl items-center justify-between gap-4 px-6">
        <a href="/" className="flex items-center gap-2.5" aria-label="Folio home">
          <img
            src="/assets/folio.png"
            alt=""
            width={30}
            height={30}
            className="rounded-lg"
          />
          <span className="text-xl font-semibold tracking-tight">
            Folio<span className="text-brand">.</span>
          </span>
        </a>
        <nav
          aria-label="Main"
          className="hidden items-center gap-7 text-sm text-muted-foreground sm:flex"
        >
          {links.map((link) => (
            <a
              key={link.href}
              href={link.href}
              className="transition-colors hover:text-foreground"
            >
              {link.label}
            </a>
          ))}
        </nav>
        <div className="flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="icon"
            onClick={toggle}
            aria-label={dark ? "Switch to light theme" : "Switch to dark theme"}
          >
            {dark ? <Sun /> : <Moon />}
          </Button>
          <Button variant="ghost" size="sm" render={<a href="/download" />}>
            Download
            <ArrowUpRight />
          </Button>
        </div>
      </div>
    </header>
  );
}
