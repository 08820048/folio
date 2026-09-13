import { useEffect, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Asterisk,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronUp,
  FileText,
  Folder,
  FolderOpen,
  GitCompare,
  PanelLeftDashed,
  Plus,
  Search,
  Settings,
  SquareSplitHorizontal,
  SquareTerminal,
  X,
} from "lucide-react";
import { Tabs, TabsList, TabsTab } from "@/components/ui/tabs";
import { useTheme } from "@/components/theme";

type View = "read" | "diff" | "terminal";
type Layout = "split" | "unified";
type Token = { c?: "k" | "f" | "s" | "c"; t: string };

const k = (t: string): Token => ({ c: "k", t });
const f = (t: string): Token => ({ c: "f", t });
const s = (t: string): Token => ({ c: "s", t });
const c = (t: string): Token => ({ c: "c", t });
const p = (t: string): Token => ({ t });

// The sample mirrors the mock project in the site copy; the syntax colors
// come from gpui-component's default theme (see index.css `.editor-mock`).
// `oldLines` is HEAD, `newLines` the working copy — a one-line change.
const newLines: Token[][] = [
  [c("// A little space to make sense of things.")],
  [p(" ")],
  [k("use"), p(" folio::{App, Theme, Workspace};")],
  [p(" ")],
  [k("mod"), p(" preview;")],
  [k("mod"), p(" tree;")],
  [p(" ")],
  [k("const"), p(" PROJECT: &"), p("str"), p(" = "), s('"paperplane"'), p(";")],
  [p(" ")],
  [k("fn"), f(" main"), p("() {")],
  [p("    tracing_subscriber::"), f("init"), p("();")],
  [p(" ")],
  [p("    "), k("let"), p(" project = Workspace::"), f("open"), p("(PROJECT)")],
  [p("        ."), f("expect"), p("("), s('"paperplane: project folder"'), p(");")],
  [p(" ")],
  [p("    "), k("let"), p(" app = App::"), f("new"), p("()")],
  [p("        ."), f("workspace"), p("(project)")],
  [p("        ."), f("theme"), p("(Theme::System)")],
  [p("        ."), f("title"), p("("), s('"Paperplane"'), p(")")],
  [p("        ."), f("run"), p("();")],
  [p(" ")],
  [p("    "), k("if"), p(" "), k("let"), p(" "), k("Err"), p("(error) = app {")],
  [p("        "), f("eprintln!"), p("("), s('"paperplane exited: {error}"'), p(");")],
  [p("    }")],
  [p("}")],
  [p(" ")],
  [c("/// Say hi to the reader.")],
  [k("fn"), f(" greet"), p("(name: &"), p("str"), p(") -> String {")],
  [p("    "), f("format!"), p("("), s('"Hello, {name}!"'), p(")")],
  [p("}")],
];

const oldLines: Token[][] = newLines.map((tokens, index) =>
  index === 17 ? [p("        ."), f("theme"), p("(Theme::Light)")] : tokens,
);

const ACTIVE_LINE = 19;
const HUNK_BEFORE = 14; // 0-based index of the first line in the hunk (line 15)
const HUNK_LABEL = "@@ -15,7 +15,7 @@";

const readmeLines = [
  "# paperplane",
  "",
  "A little space for your ideas.",
  "",
  "## Getting started",
  "",
  "Open a project.",
  "Read at your own pace.",
  "Make a small change.",
  "",
  "Your files. Your workspace.",
];

const tokenClass: Record<NonNullable<Token["c"]>, string> = {
  k: "text-[var(--mock-key)]",
  f: "text-[var(--mock-fn)]",
  s: "text-[var(--mock-string)]",
  c: "text-[var(--mock-comment)]",
};

function Tokens({ tokens }: { tokens: Token[] }) {
  return (
    <>
      {tokens.map((token, index) => (
        <span key={index} className={token.c ? tokenClass[token.c] : undefined}>
          {token.t}
        </span>
      ))}
    </>
  );
}

function CodeLine({
  number,
  tokens,
  active = false,
  caret = false,
}: {
  number: string;
  tokens: Token[];
  active?: boolean;
  caret?: boolean;
}) {
  return (
    <div className={`flex whitespace-pre ${active ? "bg-[var(--mock-active-line)]" : ""}`}>
      <span
        className={`w-12 shrink-0 pr-4 text-right text-[11px] leading-[1.6] select-none ${
          active ? "text-[var(--mock-fg)]" : "text-[var(--mock-muted)]"
        }`}
      >
        {number}
      </span>
      <code className="pr-6 font-mono text-[13px] leading-[1.6]">
        <Tokens tokens={tokens} />
        {caret && (
          <span className="ml-0.5 inline-block h-[15px] w-px translate-y-[3px] bg-[var(--mock-caret)]" />
        )}
      </code>
    </div>
  );
}

function DiffNumber({ number }: { number?: number }) {
  return (
    <span className="w-[46px] shrink-0 pr-2 text-right text-[var(--mock-muted)] select-none">
      {number ?? ""}
    </span>
  );
}

function DiffMarker({ mark }: { mark: string }) {
  return (
    <span className="flex w-[18px] shrink-0 justify-center text-[var(--mock-muted)]">
      {mark}
    </span>
  );
}

function SplitCell({
  number,
  tokens,
  change,
  old = false,
}: {
  number?: number;
  tokens: Token[];
  change?: "add" | "del";
  old?: boolean;
}) {
  return (
    <div
      className={`flex min-w-0 flex-1 items-center overflow-hidden font-mono text-[13px] ${
        change === "add"
          ? "bg-[var(--mock-add-bg)]"
          : change === "del"
            ? "bg-[var(--mock-del-bg)]"
            : ""
      }`}
      style={{ borderRight: `1px solid ${old ? "var(--mock-rule)" : "transparent"}` }}
    >
      <DiffNumber number={number} />
      <DiffMarker mark={change === "add" ? "+" : change === "del" ? "−" : " "} />
      <span className="min-w-0 flex-1 truncate whitespace-pre">
        <Tokens tokens={tokens} />
      </span>
    </div>
  );
}

function HunkRow({ label }: { label: string }) {
  return (
    <div className="flex h-6 items-center bg-[var(--mock-hover)] px-4 font-sans text-[11px] text-[var(--mock-muted)]">
      <span className="min-w-0 truncate">{label}</span>
    </div>
  );
}

function DiffChoice({
  label,
  selected = false,
  onClick,
}: {
  label: string;
  selected?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={selected}
      className={`cursor-pointer rounded px-1.5 py-0.5 text-[11px] ${
        selected
          ? "bg-[var(--mock-active)] text-[var(--mock-accent)]"
          : "text-[var(--mock-muted)]"
      }`}
    >
      {label}
    </button>
  );
}

function TreeRow({
  depth = 0,
  kind,
  name,
  modified = false,
  selected = false,
  onClick,
}: {
  depth?: number;
  kind: "folder-open" | "folder" | "file";
  name: string;
  modified?: boolean;
  selected?: boolean;
  onClick?: () => void;
}) {
  const Icon = kind === "file" ? FileText : kind === "folder-open" ? FolderOpen : Folder;
  const className = `relative flex h-[27px] w-full items-center gap-2 rounded-md pr-3 text-left text-[12px] ${
    selected
      ? "bg-[var(--mock-active)]"
      : onClick
        ? "cursor-pointer hover:bg-[var(--mock-hover)]"
        : ""
  }`;
  const content = (
    <>
      <Icon className="size-3.5 shrink-0 text-[var(--mock-muted)]" />
      <span className="min-w-0 flex-1 truncate">{name}</span>
      {modified && <Asterisk className="size-3 text-[var(--mock-modified)]" />}
      {selected && (
        <span className="absolute inset-0 rounded-md border border-[var(--mock-active-border)]" />
      )}
    </>
  );
  return onClick ? (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={selected}
      className={className}
      style={{ paddingLeft: `${12 + depth * 16}px` }}
    >
      {content}
    </button>
  ) : (
    <div className={className} style={{ paddingLeft: `${12 + depth * 16}px` }}>
      {content}
    </div>
  );
}

function RailIcon({
  icon: Icon,
  label,
  active = false,
  bottom = false,
}: {
  icon: typeof Folder;
  label: string;
  active?: boolean;
  bottom?: boolean;
}) {
  return (
    <span
      role="img"
      aria-label={label}
      className={`flex size-6 items-center justify-center rounded-md ${
        bottom ? "mt-auto" : ""
      } ${active ? "text-[var(--mock-fg)]" : "text-[var(--mock-muted)]"}`}
    >
      <Icon className="size-4" />
    </span>
  );
}

export function EditorDemo() {
  const { dark } = useTheme();
  const [view, setView] = useState<View>("read");
  const [layout, setLayout] = useState<Layout>("split");
  const [file, setFile] = useState<"main" | "readme">("main");

  // `?view=diff|terminal` deep-links a demo view; see theme.tsx for `?theme=`.
  // Applied after mount so the prerendered markup stays hydration-stable.
  useEffect(() => {
    const param = new URLSearchParams(location.search).get("view");
    if (param === "diff" || param === "terminal") setView(param);
  }, []);

  const readme = view === "read" && file === "readme";
  const relativePath = readme ? "README.md" : "src/main.rs";

  const editorBody = readme ? (
    readmeLines.map((text, index) => (
      <CodeLine key={index} number={String(index + 1)} tokens={[p(text || " ")]} />
    ))
  ) : view === "diff" ? (
    <>
      {newLines.map((tokens, index) => {
        if (index === HUNK_BEFORE) {
          return <HunkRow key="hunk" label={HUNK_LABEL} />;
        }
        if (index === 17) {
          return layout === "split" ? (
            <div key={index} className="flex h-[22px]">
              <SplitCell number={18} tokens={oldLines[17]} change="del" old />
              <SplitCell number={18} tokens={tokens} change="add" />
            </div>
          ) : (
            <div key={index}>
              <div className="flex h-[22px] items-center bg-[var(--mock-del-bg)] font-mono text-[13px]">
                <DiffNumber number={18} />
                <DiffNumber />
                <DiffMarker mark="−" />
                <span className="min-w-0 flex-1 truncate whitespace-pre">
                  <Tokens tokens={oldLines[17]} />
                </span>
              </div>
              <div className="flex h-[22px] items-center bg-[var(--mock-add-bg)] font-mono text-[13px]">
                <DiffNumber />
                <DiffNumber number={18} />
                <DiffMarker mark="+" />
                <span className="min-w-0 flex-1 truncate whitespace-pre">
                  <Tokens tokens={tokens} />
                </span>
              </div>
            </div>
          );
        }
        const line = index + 1;
        return layout === "split" ? (
          <div key={index} className="flex h-[22px]">
            <SplitCell number={line} tokens={tokens} old />
            <SplitCell number={line} tokens={tokens} />
          </div>
        ) : (
          <div key={index} className="flex h-[22px] items-center font-mono text-[13px]">
            <DiffNumber number={line} />
            <DiffNumber number={line} />
            <DiffMarker mark=" " />
            <span className="min-w-0 flex-1 truncate whitespace-pre">
              <Tokens tokens={tokens} />
            </span>
          </div>
        );
      })}
    </>
  ) : (
    newLines.map((tokens, index) => (
      <CodeLine
        key={index}
        number={String(index + 1)}
        tokens={tokens}
        active={index === ACTIVE_LINE}
        caret={index === ACTIVE_LINE}
      />
    ))
  );

  return (
    <section className="mx-auto w-full max-w-6xl px-6 pb-4" aria-label="Folio interactive demo">
      <Tabs
        value={view}
        onValueChange={(value) => {
          setView(value as View);
          setFile("main");
        }}
      >
        <TabsList>
          <TabsTab value="read">Reading</TabsTab>
          <TabsTab value="diff">Changes</TabsTab>
          <TabsTab value="terminal">Terminal</TabsTab>
        </TabsList>
      </Tabs>

      <div className="mt-4 rounded-2xl border bg-muted/40 p-6 sm:p-10">
        <div
          className={`editor-mock ${dark ? "dark" : ""} flex w-full flex-col overflow-hidden rounded-xl border shadow-2xl shadow-black/15 dark:shadow-black/50`}
          style={{ aspectRatio: "1145 / 818", background: "var(--mock-chrome)", borderColor: "var(--mock-border)" }}
        >
          {/* Title bar: traffic lights, rail toggle, project / relative path,
              dirty mark — the layout of render_titlebar in src/app.rs. */}
          <div className="flex h-10 shrink-0 items-center gap-3 px-4 text-[12px]">
            <div className="mr-1 flex gap-1.5">
              <i className="size-2.5 rounded-full bg-[var(--mock-traffic-1)]" />
              <i className="size-2.5 rounded-full bg-[var(--mock-traffic-2)]" />
              <i className="size-2.5 rounded-full bg-[var(--mock-traffic-3)]" />
            </div>
            <PanelLeftDashed className="size-4 text-[var(--mock-muted)]" />
            <span className="text-[var(--mock-fg)]">paperplane</span>
            <span className="text-[var(--mock-muted)]">/</span>
            <span className="min-w-0 flex-1 truncate text-[var(--mock-muted)]">
              {relativePath}
            </span>
            <Asterisk className="size-3.5 text-[var(--mock-accent)]" />
          </div>

          <div className="flex min-h-0 flex-1 gap-1 px-2 pb-2">
            {/* Activity rail — 36px, outside the rounded card, Settings at
                the bottom; the order of render_activity_bar. */}
            <div className="flex w-9 shrink-0 flex-col items-center gap-1 pt-2 pb-2">
              <RailIcon icon={Folder} label="File Tree" active />
              <RailIcon icon={GitCompare} label="Changes" />
              <RailIcon icon={Search} label="Search" />
              <RailIcon icon={SquareTerminal} label="Terminal" />
              <RailIcon icon={Settings} label="Settings" bottom />
            </div>

            {/* Workspace card: rounded 12, 1px border, the tree keeps its
                full height while the terminal dock hangs under the editor
                column alone. */}
            <div
              className="flex min-w-0 flex-1 overflow-hidden rounded-xl border"
              style={{ background: "var(--mock-editor-bg)", borderColor: "var(--mock-border)" }}
            >
              <aside
                aria-label="Sample project files"
                className="hidden w-[190px] shrink-0 flex-col pt-1 pb-1 sm:flex"
                style={{ background: "var(--mock-sidebar)" }}
              >
                <div className="mx-1 flex h-[42px] shrink-0 items-center gap-2 rounded-md px-3 text-[12px] text-[var(--mock-fg)]">
                  <ChevronDown className="size-3.5 text-[var(--mock-muted)]" />
                  <FolderOpen className="size-3.5 text-[var(--mock-muted)]" />
                  <span className="truncate">paperplane</span>
                </div>
                <div className="px-1">
                  <TreeRow kind="folder-open" name="src" />
                  <TreeRow depth={1} kind="file" name="main.rs" modified selected onClick={() => setFile("main")} />
                  <TreeRow depth={1} kind="file" name="workspace.rs" />
                  <TreeRow depth={1} kind="file" name="buffer.rs" />
                  <TreeRow kind="folder" name="assets" />
                  <TreeRow kind="file" name="Cargo.toml" />
                  <TreeRow kind="file" name="README.md" selected={readme} onClick={() => setFile("readme")} />
                </div>
              </aside>

              <div
                className="flex w-[5px] shrink-0 justify-center"
                style={{ background: "var(--mock-sidebar)" }}
              >
                <div className="h-full w-px" style={{ background: "var(--mock-rule)" }} />
              </div>

              <div
                className="flex min-w-0 flex-1 flex-col"
                style={{ background: "var(--mock-editor-bg)" }}
              >
                {/* Tab strip — 26px, same surface as the code, step arrows
                    on the left, split button on the right. */}
                <div className="flex h-[26px] shrink-0 items-center pl-1">
                  <span className="flex items-center text-[var(--mock-muted)]">
                    <ChevronLeft className="size-3.5" />
                    <ChevronRight className="size-3.5 opacity-40" />
                  </span>
                  <span
                    className="flex h-full items-center gap-2 border-r px-2 text-[11px] text-[var(--mock-fg)]"
                    style={{ background: "var(--mock-active)", borderColor: "var(--mock-rule)" }}
                  >
                    <span className="truncate">{readme ? "README.md" : "main.rs"}</span>
                    <X className="size-3 text-[var(--mock-muted)]" />
                  </span>
                  <span className="flex-1" />
                  <SquareSplitHorizontal className="mr-2 size-3.5 text-[var(--mock-muted)]" />
                </div>

                {view === "diff" && (
                  // The changes header of render_diff: path, counts, the
                  // HEAD/branch and Unified/Split choices, then the file and
                  // hunk stepping buttons.
                  <div
                    className="flex shrink-0 items-center gap-2 border-b px-3 py-1.5 font-sans text-[11px] text-[var(--mock-muted)]"
                    style={{ borderColor: "var(--mock-border)" }}
                  >
                    <span className="min-w-0 flex-1 truncate">{relativePath}</span>
                    <span className="whitespace-nowrap">
                      <span className="text-[var(--mock-add-fg)]">+1</span>{" "}
                      <span className="text-[var(--mock-del-fg)]">−1</span>
                    </span>
                    <DiffChoice label="HEAD" selected />
                    <DiffChoice label="main" />
                    <DiffChoice
                      label="Unified"
                      selected={layout === "unified"}
                      onClick={() => setLayout("unified")}
                    />
                    <DiffChoice
                      label="Split"
                      selected={layout === "split"}
                      onClick={() => setLayout("split")}
                    />
                    <ChevronUp className="size-3" aria-label="Previous changed file" />
                    <ChevronDown className="size-3" aria-label="Next changed file" />
                    <ArrowUp className="size-3" aria-label="Previous hunk" />
                    <ArrowDown className="size-3" aria-label="Next hunk" />
                    <X className="size-3" aria-label="Hide file changes" />
                  </div>
                )}

                <div aria-live="polite" className="min-h-0 flex-1 overflow-auto pt-2 font-mono">
                  {editorBody}
                </div>
                {/* The blame line is a row under the editor, the way
                    render_pane appends it for the focused pane. */}
                {view === "read" && (
                  <div
                    className="shrink-0 px-4 py-1 font-sans text-[11px] text-[var(--mock-muted)]"
                    style={{ background: "var(--mock-editor-bg)" }}
                  >
                    a59fa28 · xuyi, 2 hours ago — feat: ship Mac updates through Sparkle and R2
                  </div>
                )}

                {view === "terminal" && (
                  <div
                    className="shrink-0"
                    style={{ borderTop: "1px solid var(--mock-border)" }}
                  >
                    <div className="h-1" />
                    <div className="flex h-[26px] items-center pr-2">
                      <span
                        className="flex h-full items-center gap-2 border-r px-2 text-[11px] text-[var(--mock-fg)]"
                        style={{ background: "var(--mock-active)", borderColor: "var(--mock-rule)" }}
                      >
                        zsh
                        <X className="size-3 text-[var(--mock-muted)]" />
                      </span>
                      <span className="flex-1" />
                      <Plus className="size-3.5 text-[var(--mock-muted)]" aria-label="New Terminal" />
                    </div>
                    <pre className="overflow-hidden px-4 pb-3 pt-1 font-mono text-[12px] leading-relaxed">
                      <span className="text-[var(--mock-term-blue)]">➜  paperplane</span> git
                      status --short
                      {"\n"}
                      {" "}
                      <span className="text-[var(--mock-term-green)]">M</span> src/main.rs
                      {"\n\n"}
                      <span className="text-[var(--mock-term-blue)]">➜  paperplane</span>
                      <span className="ml-1 inline-block h-[13px] w-px translate-y-[2px] bg-[var(--mock-caret)]" />
                    </pre>
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="flex items-center justify-between px-1 py-4 text-xs text-muted-foreground">
        <span>↳ One window. Your project. Just enough.</span>
        <span className="max-w-40 text-right sm:max-w-none">
          Interactive demo · switch the view above
        </span>
      </div>
    </section>
  );
}
