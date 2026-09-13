import { ArrowUpRight, Download } from "lucide-react";
import { Button } from "@/components/ui/button";

export function Hero() {
  return (
    <section className="relative mx-auto w-full max-w-6xl px-6 pt-20 pb-12 text-center md:pt-28">
      <p className="mb-6 flex items-center justify-center gap-2.5 text-[11px] font-semibold tracking-[0.2em] text-muted-foreground uppercase">
        <span className="size-1.5 rounded-full bg-brand" />
        A little space for your code
      </p>
      <h1 className="text-[clamp(1.5rem,6.4vw,4.75rem)] leading-[1.12] font-semibold tracking-tight whitespace-nowrap">
        Bring <span className="text-brand">reading</span> back to code.
      </h1>
      <p className="mx-auto mt-7 max-w-lg text-base leading-relaxed text-muted-foreground">
        Open a project like opening a page.
        <br />
        Read the code, untangle the changes, make a few well-placed edits.
      </p>
      <div className="mt-9 flex flex-wrap items-center justify-center gap-6">
        <Button size="lg" render={<a href="/download" />}>
          <Download />
          Download for macOS
        </Button>
        <a
          href="https://github.com/08820048/folio"
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex h-10 items-center gap-2 whitespace-nowrap rounded-lg px-2 text-sm transition-colors hover:text-brand"
        >
          Read the source
          <ArrowUpRight className="size-4" />
        </a>
      </div>
      <p className="mt-5 text-xs text-muted-foreground">
        macOS 12+ · Apple silicon ·{" "}
        <a href="#license" className="underline underline-offset-4">
          free for noncommercial use
        </a>
      </p>
    </section>
  );
}
