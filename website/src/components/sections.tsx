import { useEffect, useState } from "react";
import {
  Check,
  FileText,
  Folder,
  GitBranch,
  PanelLeft,
  Search,
  SquareTerminal,
} from "lucide-react";
import { Accordion, AccordionItem, AccordionPanel, AccordionTrigger } from "@/components/ui/accordion";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Kbd } from "@/components/ui/kbd";
import { Download } from "lucide-react";

export function Principles() {
  return (
    <div className="border-y">
      <div className="mx-auto flex w-full max-w-6xl flex-wrap items-center justify-center gap-x-14 gap-y-4 px-6 py-10 text-sm text-muted-foreground sm:gap-x-16">
        <span className="flex items-center gap-2.5">
          <Folder className="size-4 text-brand" />
          Files always stay local
        </span>
        <span className="flex items-center gap-2.5">
          <PanelLeft className="size-4 text-brand" />
          A truly native desktop feel
        </span>
        <span className="flex items-center gap-2.5">
          <Check className="size-4 text-brand" />
          No account, ready on launch
        </span>
      </div>
    </div>
  );
}

const features = [
  {
    number: "01",
    icon: FileText,
    title: "Reading, at your own pace.",
    body: "Clear syntax highlighting, code folding, and split views. Collapse the sidebar and let the code unfold.",
    tags: ["60 languages highlighted", "Side-by-side or stacked", "Light & dark"],
  },
  {
    number: "02",
    icon: Search,
    title: "Small edits, made easy.",
    body: "Multi-cursor, find & replace, bracket pairing, undo. Editing stays light, and saving takes every change seriously.",
    tags: ["Multi-cursor editing", "Project-wide search", "Save conflict checks"],
  },
  {
    number: "03",
    icon: GitBranch,
    title: "The full story, in view.",
    body: "See a file's changes against HEAD, trace who wrote the line you're on. Need a command? Pull up the terminal below.",
    tags: ["Git diff", "Line-by-line blame", "Project terminal"],
  },
];

export function Features() {
  return (
    <section
      id="features"
      aria-labelledby="features-title"
      className="mx-auto grid w-full max-w-6xl gap-12 px-6 py-24 md:grid-cols-[0.9fr_1.1fr] md:gap-16 lg:py-32"
    >
      <div>
        <Badge variant="outline" className="mb-5 tracking-[0.18em] uppercase">
          Built for the in-between
        </Badge>
        <h2
          id="features-title"
          className="text-3xl leading-snug font-medium tracking-tight text-balance md:text-4xl"
        >
          Sometimes, you just want
          <br />
          a good look at the code.
        </h2>
        <p className="mt-6 text-sm leading-loose text-muted-foreground">
          An AI-written project, a repo you haven't opened in months,
          <br />
          or a config that needs two lines changed.
          <br />
          Folio keeps a quiet space for those moments.
        </p>
      </div>
      <div>
        {features.map((feature) => (
          <article
            key={feature.number}
            className="flex gap-5 border-b py-7 first:pt-0 last:border-b-0 last:pb-0"
          >
            <span className="shrink-0 font-mono text-xs text-brand">
              {feature.number}
            </span>
            <div className="min-w-0">
              <h3 className="text-base font-medium">{feature.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                {feature.body}
              </p>
              <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
                {feature.tags.map((tag) => (
                  <span key={tag}>· {tag}</span>
                ))}
              </div>
            </div>
            <feature.icon className="ml-auto size-5 shrink-0 text-brand" />
          </article>
        ))}
      </div>
    </section>
  );
}

const steps = [
  {
    label: "01 / OPEN",
    keys: "⌘ O",
    title: "Open the project",
    body: "Pick a local folder. Several projects share one window, and switching keeps each one's state.",
  },
  {
    label: "02 / FIND",
    keys: "⌘ P",
    title: "Find that line",
    body: "Jump by filename with quick open, search for the context you need. Two files can sit side by side.",
  },
  {
    label: "03 / KEEP",
    keys: "⌘ S",
    title: "Keep your thoughts",
    body: "Edit a few spots and save. Next time, the projects and tabs return where you left them.",
  },
];

export function Workflow() {
  return (
    <section id="workflow" aria-labelledby="workflow-title" className="border-y bg-muted/40">
      <div className="mx-auto w-full max-w-6xl px-6 py-20 lg:py-24">
        <div className="grid gap-6 md:grid-cols-2">
          <div>
            <Badge variant="outline" className="mb-5 tracking-[0.18em] uppercase">
              From open to understood
            </Badge>
            <h2
              id="workflow-title"
              className="text-3xl leading-snug font-medium tracking-tight md:text-4xl"
            >
              From opening
              <br />
              to understood.
            </h2>
          </div>
          <p className="self-end text-sm text-muted-foreground md:pb-1">
            Use the shortcuts you already know, and keep your attention on the code.
          </p>
        </div>
        <div className="mt-12 grid gap-10 md:grid-cols-3">
          {steps.map((step) => (
            <article key={step.label} className="border-t pt-6">
              <div className="flex items-center justify-between font-mono text-xs text-muted-foreground">
                <span>{step.label}</span>
                <Kbd>{step.keys}</Kbd>
              </div>
              <h3 className="mt-5 text-base font-medium">{step.title}</h3>
              <p className="mt-2.5 max-w-70 text-sm leading-relaxed text-muted-foreground">
                {step.body}
              </p>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}

const details = [
  {
    icon: PanelLeft,
    title: "Native feel",
    body: "Built with Rust + GPUI — the interface is drawn directly, no browser engine.",
  },
  {
    icon: Folder,
    title: "Your own files",
    body: "Open local repositories. Reading and editing need no cloud and no sign-in.",
  },
  {
    icon: Check,
    title: "It remembers",
    body: "Projects, tabs, and unsaved edits come back, so an interruption never breaks the thread.",
  },
  {
    icon: SquareTerminal,
    title: "Keeps the needed tools",
    body: "The terminal lives below the workspace. Open it when needed; commands keep running when it's closed.",
  },
];

export function Details() {
  return (
    <section className="border-b">
      <div className="mx-auto grid w-full max-w-6xl gap-10 px-6 py-24 md:grid-cols-[0.75fr_1.25fr] lg:py-28">
        <div>
          <Badge variant="outline" className="mb-5 tracking-[0.18em] uppercase">
            Small by design
          </Badge>
          <h2 className="text-3xl leading-snug font-medium tracking-tight md:text-4xl">
            Lighter,
            <br />
            on purpose.
          </h2>
        </div>
        <div className="grid gap-x-11 gap-y-9 sm:grid-cols-2">
          {details.map((detail) => (
            <article key={detail.title}>
              <detail.icon className="mb-3 size-5 text-brand" />
              <h3 className="text-[15px] font-medium">{detail.title}</h3>
              <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
                {detail.body}
              </p>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}

const faqs = [
  {
    id: "fit",
    question: "What is Folio good for?",
    answer: (
      <>
        Reading local code, reviewing AI-generated projects, checking Git changes, and editing
        configs or small bits of logic. It focuses on reading and light editing — no debugger,
        plugin marketplace, or LSP like a full IDE.
      </>
    ),
  },
  {
    id: "mac",
    question: "Which Macs are supported?",
    answer: (
      <>
        Apple silicon (M-series) builds, requiring macOS 12 or newer. Intel Macs, Windows, and
        Linux have no official builds yet.
      </>
    ),
  },
  {
    id: "license",
    question: "Is it free to use?",
    answer: (
      <>
        Yes, Folio is free — and I hope she stays free forever!
      </>
    ),
  },
  {
    id: "privacy",
    question: "Does it upload my code? Do I need an AI account?",
    answer: (
      <>
        Folio reads and writes files locally, ships no cloud AI, needs no account, and never
        uploads your project in order to read or edit it. The macOS app's update check contacts
        the update service; commands you run in the terminal are your own.
      </>
    ),
  },
  {
    id: "install",
    question: "How do I install and update?",
    answer: (
      <>
        Open the DMG and drag Folio into Applications. Later, use Check for Updates in the app
        menu.
      </>
    ),
  },
];

export function Faq() {
  // Controlled so a direct #license link opens its answer, also after
  // hydration — the prerendered HTML always starts closed.
  const [open, setOpen] = useState<string[]>([]);
  useEffect(() => {
    const reveal = () => {
      if (location.hash === "#license") setOpen(["license"]);
    };
    reveal();
    window.addEventListener("hashchange", reveal);
    return () => window.removeEventListener("hashchange", reveal);
  }, []);

  return (
    <section
      id="faq"
      aria-labelledby="faq-title"
      className="mx-auto grid w-full max-w-6xl gap-10 px-6 py-24 md:grid-cols-[0.75fr_1.25fr] lg:py-28"
    >
      <div>
        <Badge variant="outline" className="mb-5 tracking-[0.18em] uppercase">
          A few things to know
        </Badge>
        <h2 id="faq-title" className="text-3xl font-medium tracking-tight">
          You might also wonder.
        </h2>
      </div>
      <Accordion
        value={open}
        onValueChange={(value) => setOpen(value as string[])}
      >
        {faqs.map((faq) => (
          <AccordionItem key={faq.id} id={faq.id} value={faq.id}>
            <AccordionTrigger>{faq.question}</AccordionTrigger>
            <AccordionPanel className="max-w-xl leading-loose">
              {faq.answer}
            </AccordionPanel>
          </AccordionItem>
        ))}
      </Accordion>
    </section>
  );
}

export function FinalCta() {
  return (
    <section id="download" className="mx-auto w-full max-w-6xl px-6 pb-16">
      <div className="flex flex-col items-start gap-6 rounded-2xl bg-[#002526] p-8 text-[#eef6f1] sm:flex-row sm:items-center sm:justify-between sm:p-11">
        <div className="flex items-center gap-7">
          <img
            src="/assets/folio.png"
            alt="Folio app icon"
            width={76}
            height={76}
            loading="lazy"
            className="hidden rounded-2xl shadow-lg shadow-black/40 min-[420px]:block"
          />
          <div>
            <p className="mb-2.5 text-[8px] font-semibold tracking-[0.18em] text-[#8db3a9] uppercase">
              Your code. A quieter place.
            </p>
            <h2 className="text-2xl leading-snug font-medium tracking-tight sm:text-3xl">
              Your next project,
              <br />
              opened gently.
            </h2>
          </div>
        </div>
        <div className="w-full sm:w-auto">
          <Button
            size="lg"
            render={<a href="/download" />}
            className="w-full bg-[#8de3bc] text-[#05231c] inset-shadow-[0_1px_--theme(--color-white/50%)] hover:bg-[#a9edcf] sm:w-auto"
          >
            <Download />
            Download Folio for macOS
          </Button>
          <p className="mt-3 text-center text-[10px] text-[#8db3a9]">
            macOS 12+ · Apple silicon
          </p>
        </div>
      </div>
    </section>
  );
}

export function SiteFooter() {
  return (
    <footer className="border-t">
      <div className="mx-auto flex w-full max-w-6xl flex-wrap items-center justify-between gap-4 px-6 py-8">
        <div className="flex items-center gap-4 text-xs text-muted-foreground">
          <a href="/" className="text-xl font-semibold tracking-tight text-foreground">
            Folio.
          </a>
          <span>A little space for reading code.</span>
        </div>
        <div className="flex items-center gap-4">
          <a
            href="https://x.com/shizouffa"
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Folio on X"
            className="text-muted-foreground transition-colors hover:text-foreground"
          >
            <XLogo />
          </a>
          <small className="text-xs text-muted-foreground">© 2026 Folio</small>
        </div>
      </div>
    </footer>
  );
}

function XLogo() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className="size-4" aria-hidden="true">
      <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
    </svg>
  );
}
