import { Download, Plus, Sparkles, Wrench } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SiteFooter } from "@/components/sections";
import { SiteHeader } from "@/components/site-header";
import { ThemeProvider } from "@/components/theme";
import {
  formatReleaseDate,
  latestRelease,
  releases,
  type Release,
  type ReleaseKind,
} from "@/data/releases";

const groups: { kind: ReleaseKind; label: string; icon: typeof Plus }[] = [
  { kind: "added", label: "Added", icon: Plus },
  { kind: "improved", label: "Improved", icon: Sparkles },
  { kind: "fixed", label: "Fixed", icon: Wrench },
];

function ReleaseEntry({ release }: { release: Release }) {
  return (
    <article
      id={`v${release.version}`}
      aria-labelledby={`v${release.version}-title`}
      className="scroll-mt-24 border-b py-12 first:pt-0 last:border-b-0 last:pb-0"
    >
      <header className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <h2
          id={`v${release.version}-title`}
          className="font-mono text-2xl font-medium tracking-tight"
        >
          v{release.version}
        </h2>
        {release.latest && (
          <Badge
            variant="outline"
            size="sm"
            className="border-brand/30 bg-brand/8 tracking-[0.12em] text-brand uppercase"
          >
            Latest
          </Badge>
        )}
        <time dateTime={release.date} className="text-xs text-muted-foreground">
          {formatReleaseDate(release.date)}
        </time>
        {release.build !== undefined && (
          <span className="text-xs text-muted-foreground">
            · build {release.build}
          </span>
        )}
      </header>

      {release.summary && (
        <p className="mt-4 max-w-xl text-sm leading-loose text-muted-foreground">
          {release.summary}
        </p>
      )}

      <div className="mt-7 space-y-7">
        {groups.map(({ kind, label, icon: Icon }) => {
          const notes = release.notes.filter((note) => note.kind === kind);
          if (notes.length === 0) return null;
          return (
            <div key={kind}>
              <p className="mb-3 flex items-center gap-2 text-[11px] font-semibold tracking-[0.16em] text-brand uppercase">
                <Icon className="size-3.5" />
                {label}
              </p>
              <ul className="space-y-2.5">
                {notes.map((note) => (
                  <li key={note.text} className="flex gap-3">
                    <span className="mt-2.5 size-1 shrink-0 rounded-full bg-muted-foreground/50" />
                    <span className="text-sm leading-relaxed">{note.text}</span>
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
      </div>
    </article>
  );
}

export function UpdatesPage() {
  return (
    <ThemeProvider>
      <a
        href="#main"
        className="fixed top-3 left-4 z-50 -translate-y-20 rounded-md bg-brand px-4 py-2.5 text-sm text-primary-foreground transition-transform focus-visible:translate-y-0"
      >
        Skip to content
      </a>
      <SiteHeader page="updates" />
      <main id="main">
        <section className="mx-auto w-full max-w-6xl px-6 pt-20 pb-14 md:pt-24">
          <Badge variant="outline" className="mb-5 tracking-[0.18em] uppercase">
            Changelog
          </Badge>
          <h1 className="max-w-2xl text-4xl leading-tight font-semibold tracking-tight text-balance md:text-5xl">
            Every version, and what changed.
          </h1>
          <p className="mt-7 max-w-xl text-base leading-relaxed text-muted-foreground">
            Once installed, Folio keeps itself up to date through Sparkle.
            Every release is listed here, newest first, with the parts worth
            knowing.
          </p>
          <div className="mt-9 flex flex-wrap items-center gap-x-6 gap-y-3">
            <Button size="lg" render={<a href="/download" />}>
              <Download />
              Download for macOS
            </Button>
            <span className="text-xs text-muted-foreground">
              Latest · v{latestRelease.version} ·{" "}
              {formatReleaseDate(latestRelease.date)}
            </span>
          </div>
        </section>

        <section className="mx-auto grid w-full max-w-6xl gap-12 px-6 pb-24 md:grid-cols-[0.55fr_1.45fr] md:gap-16">
          <nav aria-label="Versions" className="hidden md:block">
            <div className="sticky top-24">
              <p className="mb-4 text-[11px] font-semibold tracking-[0.18em] text-muted-foreground uppercase">
                Versions
              </p>
              <ul className="border-l">
                {releases.map((release) => (
                  <li key={release.version}>
                    <a
                      href={`#v${release.version}`}
                      className="-ml-px flex items-center gap-2 border-l-2 border-transparent py-1.5 pl-3 font-mono text-sm text-muted-foreground transition-colors hover:border-brand hover:text-foreground"
                    >
                      v{release.version}
                      {release.latest && (
                        <span className="size-1.5 rounded-full bg-brand" />
                      )}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          </nav>
          <div>
            {releases.map((release) => (
              <ReleaseEntry key={release.version} release={release} />
            ))}
          </div>
        </section>
      </main>
      <SiteFooter page="updates" />
    </ThemeProvider>
  );
}
