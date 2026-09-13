import { Download } from "lucide-react";
import { Button } from "@/components/ui/button";
import { TypewriterEffectSmooth } from "@/components/ui/typewriter-effect";

export function Hero() {
  return (
    <section className="relative mx-auto w-full max-w-6xl px-6 pt-20 pb-12 text-center md:pt-28">
      <h1 className="text-[clamp(1.5rem,6.4vw,4.75rem)] leading-[1.15] font-semibold tracking-tight">
        <TypewriterEffectSmooth
          words={[
            { text: "Bring" },
            { text: "reading", className: "text-brand" },
            { text: "back" },
            { text: "to" },
            { text: "code." },
          ]}
        />
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
      </div>
    </section>
  );
}
