/**
 * Structural lint for the repo skills and the drive-app feature map.
 *
 * Pure functions only: nothing here exits, prints, touches the network or reads anything outside the
 * root it is given. `cli.ts` is the command line; `lint.test.ts` exercises every rule on fixture trees.
 *
 * Catalogs (driver verbs, rize commands, pnpm scripts) are parsed from the real sources on every run so
 * the docs can never drift from the tools they describe.
 */
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, posix } from "node:path";

export interface Finding {
  /** Repo-relative path, forward slashes. */
  file: string;
  /** 1-based. */
  line: number;
  rule: string;
  message: string;
}

export const SKILLS_DIR = ".agents/skills";
export const FEATURES_DIR = `${SKILLS_DIR}/drive-app/references/features`;
const COMMANDS_MD = "src-tauri/cli/COMMANDS.md";
const TYPES_TS = "src/lib/types.ts";
const DRIVE_SH = "scripts/app-drive.sh";
const DRIVE_WIN = "scripts/app-drive-windows.mjs";

export const SURFACES = [
  "window",
  "pulse",
  "reminder",
  "tray",
  "cli",
  "background",
] as const;
export const REACH = ["full", "partial", "none"] as const;
const FEATURE_KEYS = [
  "id",
  "title",
  "summary",
  "surfaces",
  "routes",
  "code",
  "rize",
  "reach-macos",
  "reach-windows",
  "reach-rize",
  "verified",
] as const;
const VERIFIED_RE =
  /^(never|\d{4}-\d{2}-\d{2} (macos-drive|windows-drive|rize))$/;
const FEATURE_SECTIONS = [
  "Sub-features",
  "How to get to it",
  "Driving it",
  "What proves it works",
  "Gotchas",
  "Not reachable",
] as const;
const INDEX_HEADER =
  "| Feature | What it is | Surfaces | Reach (macOS / Windows / rize) | Verified |";
const INDEX_NOT_REACHABLE = "Not reachable by any tool here";

// The `path` rule is limited to the verification skills and the feature map: other skills may name
// paths that live outside this repository in prose, which this check cannot resolve.
const PATH_RULE_SCOPES = [
  `${SKILLS_DIR}/drive-app/`,
  `${SKILLS_DIR}/drive-app-windows/`,
];

const IDENT_RE = /^[a-z][a-z0-9-]*$/;
const KEBAB_RE = /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/;
const REPO_PATH_RE =
  /^(\.agents|\.github|scripts|src|src-tauri|docs|tests)\/[A-Za-z0-9_./-]+$/;
const PNPM_BUILTINS = new Set(["install", "exec", "run", "dlx"]);
const RIZE_GLOBAL_FLAGS = new Set([
  "--json",
  "--full",
  "--yes",
  "-y",
  "--dry-run",
]);

// ---------------------------------------------------------------- filesystem helpers

function readText(root: string, rel: string): string | null {
  try {
    return readFileSync(join(root, rel), "utf8").replace(/\r\n/g, "\n");
  } catch {
    return null;
  }
}

function isDirectory(root: string, rel: string): boolean {
  try {
    return statSync(join(root, rel)).isDirectory();
  } catch {
    return false;
  }
}

function listDir(root: string, rel: string): string[] {
  try {
    return readdirSync(join(root, rel)).sort();
  } catch {
    return [];
  }
}

function markdownFiles(root: string, rel: string): string[] {
  const out: string[] = [];
  for (const name of listDir(root, rel)) {
    const child = posix.join(rel, name);
    if (isDirectory(root, child)) out.push(...markdownFiles(root, child));
    else if (name.endsWith(".md")) out.push(child);
  }
  return out;
}

/** Skill directory names that hold a SKILL.md, sorted. */
export function listSkills(root: string): string[] {
  return listDir(root, SKILLS_DIR).filter((dir) =>
    existsSync(join(root, SKILLS_DIR, dir, "SKILL.md")),
  );
}

// ---------------------------------------------------------------- markdown scanning

export interface MdLine {
  n: number;
  text: string;
  /** `fence` is a line inside a fenced block, `marker` an opening or closing fence line. */
  kind: "text" | "fence" | "marker";
}

export function scanLines(text: string): MdLine[] {
  const lines = text.split("\n");
  const out: MdLine[] = [];
  let open: { char: string; length: number } | null = null;
  lines.forEach((line, index) => {
    const n = index + 1;
    const match = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line);
    if (open) {
      if (
        match &&
        match[1][0] === open.char &&
        match[1].length >= open.length &&
        match[2].trim() === ""
      ) {
        open = null;
        out.push({ n, text: line, kind: "marker" });
      } else {
        out.push({ n, text: line, kind: "fence" });
      }
    } else if (match && !(match[1][0] === "`" && match[2].includes("`"))) {
      open = { char: match[1][0], length: match[1].length };
      out.push({ n, text: line, kind: "marker" });
    } else {
      out.push({ n, text: line, kind: "text" });
    }
  });
  return out;
}

interface CodeSpan {
  text: string;
  start: number;
  end: number;
}

/** Inline code spans on one line (a run of N backticks closes at the next run of exactly N). */
export function codeSpans(line: string): CodeSpan[] {
  const spans: CodeSpan[] = [];
  let i = 0;
  while (i < line.length) {
    if (line[i] !== "`") {
      i++;
      continue;
    }
    let run = 0;
    while (line[i + run] === "`") run++;
    let j = i + run;
    let close = -1;
    while (j < line.length) {
      if (line[j] !== "`") {
        j++;
        continue;
      }
      let length = 0;
      while (line[j + length] === "`") length++;
      if (length === run) {
        close = j;
        break;
      }
      j += length;
    }
    if (close === -1) {
      i += run;
      continue;
    }
    spans.push({
      text: line.slice(i + run, close).trim(),
      start: i,
      end: close + run,
    });
    i = close + run;
  }
  return spans;
}

// ---------------------------------------------------------------- command catalog

export interface Catalog {
  /** Verbs in `scripts/app-drive.sh` usage(). */
  shVerbs: Set<string>;
  /** Verbs in `scripts/app-drive-windows.mjs` USAGE. */
  winVerbs: Set<string>;
  /** Shipped rize top-level commands and their subcommands. */
  rize: Map<string, Set<string>>;
  /** Non-shipped rize commands: "group" or "group sub" mapped to the Status cell. */
  rizePlanned: Map<string, string>;
  scripts: Set<string>;
}

/** Driver verbs: the two-space-indented command lines of the usage text, before its "Options" block. */
export function parseUsageVerbs(source: string, anchor: RegExp): Set<string> {
  const verbs = new Set<string>();
  const at = source.search(anchor);
  if (at === -1) return verbs;
  const lines = source.slice(at).split("\n");
  const first = lines.findIndex((line) => line.includes("usage:"));
  if (first === -1) return verbs;
  for (const line of lines.slice(first + 1)) {
    if (/^Options/.test(line)) break;
    const match = /^ {2}([a-z][a-z0-9-]*)(?=\s|$)/.exec(line);
    if (match) verbs.add(match[1]);
  }
  return verbs;
}

function splitCells(row: string): string[] {
  return row
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split(/(?<!\\)\|/)
    .map((cell) => cell.trim());
}

export function parseRizeCatalog(markdown: string): {
  shipped: Map<string, Set<string>>;
  planned: Map<string, string>;
} {
  const shipped = new Map<string, Set<string>>();
  const planned = new Map<string, string>();
  for (const line of markdown.split("\n")) {
    if (!line.startsWith("|")) continue;
    const cells = splitCells(line);
    if (cells.length < 2) continue;
    const status = cells[cells.length - 1];
    const segments = [...cells[0].matchAll(/`([^`]+)`/g)].map((m) => m[1]);
    let group: string | null = null;
    const record = (g: string, sub: string | null) => {
      if (status === "Shipped") {
        if (!shipped.has(g)) shipped.set(g, new Set());
        if (sub) shipped.get(g)?.add(sub);
      } else {
        planned.set(g, status);
        if (sub) planned.set(`${g} ${sub}`, status);
      }
    };
    for (const segment of segments) {
      const words = segment.trim().split(/\s+/);
      if (words[0] === "rize") {
        const g = words[1];
        if (g && IDENT_RE.test(g)) {
          group = g;
          const sub = words[2];
          record(g, sub && IDENT_RE.test(sub) ? sub : null);
        } else {
          group = null;
        }
      } else if (group && !/^[-<]/.test(segment)) {
        if (IDENT_RE.test(words[0])) record(group, words[0]);
      }
    }
  }
  return { shipped, planned };
}

export function loadCatalog(root: string): Catalog {
  const sh = readText(root, DRIVE_SH) ?? "";
  const win = readText(root, DRIVE_WIN) ?? "";
  const rize = parseRizeCatalog(readText(root, COMMANDS_MD) ?? "");
  const scripts = new Set<string>();
  try {
    const pkg = JSON.parse(readText(root, "package.json") ?? "{}") as {
      scripts?: Record<string, string>;
    };
    for (const name of Object.keys(pkg.scripts ?? {})) scripts.add(name);
  } catch {
    // an unparsable package.json leaves no scripts; every pnpm reference then fails loudly
  }
  return {
    shVerbs: parseUsageVerbs(sh, /^usage\(\)\s*\{/m),
    winVerbs: parseUsageVerbs(win, /^const USAGE = `/m),
    rize: rize.shipped,
    rizePlanned: rize.planned,
    scripts,
  };
}

// ---------------------------------------------------------------- shell-ish tokenizer

export interface Token {
  text: string;
  kind: "word" | "quoted" | "op";
}

/** Splits one line of shell (or one inline code span) into words, quoted words and operators. */
export function tokenize(source: string): Token[] {
  const tokens: Token[] = [];
  let current = "";
  let has = false;
  let quoted = false;
  const flush = () => {
    if (has) tokens.push({ text: current, kind: quoted ? "quoted" : "word" });
    current = "";
    has = false;
    quoted = false;
  };
  for (let i = 0; i < source.length; i++) {
    const ch = source[i];
    if (ch === " " || ch === "\t") {
      flush();
    } else if (ch === "#" && !has) {
      break;
    } else if (ch === "\\") {
      if (i + 1 < source.length) {
        current += source[++i];
        has = true;
      }
    } else if (ch === '"' || ch === "'") {
      has = true;
      quoted = true;
      i++;
      while (i < source.length && source[i] !== ch) {
        if (ch === '"' && source[i] === "\\" && i + 1 < source.length) i++;
        current += source[i++];
      }
    } else if ("|&;<>()`".includes(ch)) {
      if (ch === "(" && current.endsWith("$")) current = current.slice(0, -1);
      flush();
      tokens.push({ text: ch, kind: "op" });
    } else {
      current += ch;
      has = true;
    }
  }
  flush();
  return tokens;
}

const COMMAND_PREFIXES = new Set([
  "then",
  "do",
  "else",
  "exec",
  "time",
  "sudo",
  "command",
  "$",
]);

function atCommandPosition(tokens: Token[], i: number): boolean {
  if (i === 0) return true;
  const prev = tokens[i - 1];
  if (prev.kind === "op") return true;
  if (prev.kind !== "word") return false;
  return (
    /^[A-Za-z_][A-Za-z0-9_]*=/.test(prev.text) ||
    COMMAND_PREFIXES.has(prev.text)
  );
}

/** Up to two command words after `rize`, skipping its global flags; stops at the first non-word. */
function readRizeWords(tokens: Token[], from: number): string[] {
  const words: string[] = [];
  let i = from;
  while (i < tokens.length && words.length < 2) {
    const token = tokens[i];
    if (token.kind !== "word") break;
    if (RIZE_GLOBAL_FLAGS.has(token.text)) {
      i++;
    } else if (token.text === "--data-dir") {
      i += 2;
    } else if (token.text.startsWith("--data-dir=")) {
      i++;
    } else if (IDENT_RE.test(token.text)) {
      words.push(token.text);
      i++;
    } else {
      break;
    }
  }
  return words;
}

function plannedNote(catalog: Catalog, key: string): string | null {
  const status = catalog.rizePlanned.get(key);
  return status
    ? `is planned (status ${status}), not shipped, in ${COMMANDS_MD}; do not document it as usable until it ships`
    : null;
}

/** Why `rize <words>` is not a Shipped command, or null when it is. */
export function rizeProblem(words: string[], catalog: Catalog): string | null {
  if (words.length === 0) return null;
  const [group, sub] = words;
  const subs = catalog.rize.get(group);
  if (!subs) {
    const planned = plannedNote(catalog, group);
    return planned
      ? `rize ${group} ${planned}`
      : `"${group}" is not a rize command in ${COMMANDS_MD} (${[...catalog.rize.keys()].join(", ")}); use one of those`;
  }
  if (sub && subs.size > 0 && !subs.has(sub)) {
    const planned = plannedNote(catalog, `${group} ${sub}`);
    return planned
      ? `rize ${group} ${sub} ${planned}`
      : `"${sub}" is not a shipped subcommand of rize ${group} in ${COMMANDS_MD} (${[...subs].join(", ")}); use one of those`;
  }
  return null;
}

function pnpmProblem(name: string, catalog: Catalog): string | null {
  if (PNPM_BUILTINS.has(name) || catalog.scripts.has(name)) return null;
  return `"${name}" is not a package.json script or a pnpm built-in (${[...PNPM_BUILTINS].join(", ")}); use one of the scripts: ${[...catalog.scripts].join(", ")}`;
}

function driverKind(word: string): "sh" | "win" | null {
  if (word === DRIVE_SH || word.endsWith(`/${DRIVE_SH}`)) return "sh";
  if (word === DRIVE_WIN || word.endsWith(`/${DRIVE_WIN}`)) return "win";
  return null;
}

/** Every command-reference problem in one tokenized line or span. */
export function commandProblems(tokens: Token[], catalog: Catalog): string[] {
  const out: string[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (token.kind !== "word") continue;
    const driver = driverKind(token.text);
    if (driver) {
      const verb = tokens[i + 1];
      if (verb?.kind === "word" && IDENT_RE.test(verb.text)) {
        const verbs = driver === "sh" ? catalog.shVerbs : catalog.winVerbs;
        const script = driver === "sh" ? DRIVE_SH : DRIVE_WIN;
        if (!verbs.has(verb.text)) {
          out.push(
            `"${verb.text}" is not a verb in the ${script} usage text (${[...verbs].join(", ")}); use one of those`,
          );
        } else if (verb.text === "rize") {
          let j = i + 2;
          while (
            tokens[j]?.kind === "word" &&
            (tokens[j].text === "-s" || tokens[j].text === "--session")
          ) {
            j += 2;
          }
          if (tokens[j]?.kind === "word" && tokens[j].text === "--") {
            const problem = rizeProblem(readRizeWords(tokens, j + 1), catalog);
            if (problem) out.push(problem);
          }
        }
        i++;
      }
      continue;
    }
    if (!atCommandPosition(tokens, i)) continue;
    if (token.text === "rize") {
      const problem = rizeProblem(readRizeWords(tokens, i + 1), catalog);
      if (problem) out.push(problem);
    } else if (token.text === "pnpm") {
      let j = i + 1;
      while (tokens[j]?.kind === "word" && tokens[j].text.startsWith("-")) {
        j += ["--dir", "-C", "--filter", "-F"].includes(tokens[j].text) ? 2 : 1;
      }
      const name = tokens[j];
      if (name?.kind === "word" && /^[A-Za-z0-9][\w:.-]*$/.test(name.text)) {
        const problem = pnpmProblem(name.text, catalog);
        if (problem) out.push(problem);
      }
    }
  }
  return out;
}

// ---------------------------------------------------------------- generic markdown rules

function finding(
  file: string,
  line: number,
  rule: string,
  message: string,
): Finding {
  return { file, line, rule, message };
}

/** `link`: relative markdown links must resolve. */
export function ruleLink(root: string, file: string, text: string): Finding[] {
  const out: Finding[] = [];
  for (const line of scanLines(text)) {
    if (line.kind !== "text") continue;
    let masked = line.text;
    for (const span of codeSpans(line.text)) {
      masked =
        masked.slice(0, span.start) +
        " ".repeat(span.end - span.start) +
        masked.slice(span.end);
    }
    for (const match of masked.matchAll(
      /\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g,
    )) {
      const target = match[1];
      if (/^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith("#"))
        continue;
      const clean = target.replace(/[#?].*$/, "");
      if (!clean) continue;
      const resolved = clean.startsWith("/")
        ? posix.normalize(clean.slice(1))
        : posix.normalize(posix.join(posix.dirname(file), clean));
      if (!existsSync(join(root, resolved))) {
        out.push(
          finding(
            file,
            line.n,
            "link",
            `link target "${target}" does not exist (looked for ${resolved}); fix the path or remove the link`,
          ),
        );
      }
    }
  }
  return out;
}

/** `path`: an inline-code span that is a repo path must exist. */
export function rulePath(root: string, file: string, text: string): Finding[] {
  const out: Finding[] = [];
  for (const line of scanLines(text)) {
    if (line.kind !== "text") continue;
    for (const span of codeSpans(line.text)) {
      if (!REPO_PATH_RE.test(span.text)) continue;
      const path = span.text.replace(/\/$/, "");
      if (!existsSync(join(root, path))) {
        out.push(
          finding(
            file,
            line.n,
            "path",
            `\`${span.text}\` does not exist in the repo; fix the path or remove the reference`,
          ),
        );
      }
    }
  }
  return out;
}

/** `placeholder`: no unfinished-work markers. */
export function rulePlaceholder(file: string, text: string): Finding[] {
  const out: Finding[] = [];
  text.split("\n").forEach((line, index) => {
    const marker = /\b(TODO|TBD|FIXME|XXX)\b/.exec(line);
    const lorem = /lorem ipsum/i.exec(line);
    const braces = /\{\{.*?\}\}/.exec(line);
    const hit = marker?.[0] ?? lorem?.[0] ?? braces?.[0];
    if (hit) {
      out.push(
        finding(
          file,
          index + 1,
          "placeholder",
          `placeholder "${hit}" left in the text; write the real content or delete the line`,
        ),
      );
    }
  });
  return out;
}

/** `command`: driver, rize and pnpm references in code must exist in the real catalogs. */
export function ruleCommand(
  catalog: Catalog,
  file: string,
  text: string,
): Finding[] {
  const out: Finding[] = [];
  for (const line of scanLines(text)) {
    if (line.kind === "marker") continue;
    const sources =
      line.kind === "fence"
        ? [line.text]
        : codeSpans(line.text).map((span) => span.text);
    for (const source of sources) {
      for (const message of commandProblems(tokenize(source), catalog)) {
        out.push(finding(file, line.n, "command", message));
      }
    }
  }
  return out;
}

// ---------------------------------------------------------------- skill frontmatter

interface RawField {
  key: string;
  value: string;
  line: number;
}

function splitFrontmatter(lines: string[]): { close: number } | null {
  if (lines[0] !== "---") return null;
  for (let i = 1; i < lines.length; i++) {
    if (lines[i] === "---") return { close: i };
  }
  return null;
}

function unquote(value: string): string {
  const match = /^(["'])(.*)\1$/.exec(value);
  return match ? match[2] : value;
}

/** Loose `key: value` reader for SKILL.md: tolerates quoting and folded or indented continuations. */
function readSkillFields(lines: string[], close: number): RawField[] {
  const fields: RawField[] = [];
  for (let i = 1; i < close; i++) {
    const match = /^([A-Za-z0-9_-]+):\s*(.*)$/.exec(lines[i]);
    if (match) {
      fields.push({ key: match[1], value: match[2].trim(), line: i + 1 });
    } else if (/^\s+\S/.test(lines[i]) && fields.length > 0) {
      const last = fields[fields.length - 1];
      last.value = `${last.value} ${lines[i].trim()}`.trim();
    }
  }
  for (const field of fields) {
    field.value = /^[>|][+-]?$/.test(field.value)
      ? ""
      : unquote(field.value.replace(/^[>|][+-]?\s+/, ""));
  }
  return fields;
}

/** `frontmatter`: SKILL.md header. */
export function ruleSkillFrontmatter(
  file: string,
  dir: string,
  text: string,
): Finding[] {
  const out: Finding[] = [];
  const lines = text.split("\n");
  const block = splitFrontmatter(lines);
  if (!block) {
    return [
      finding(
        file,
        1,
        "frontmatter",
        lines[0] === "---"
          ? "frontmatter block is never closed; end it with a line that is exactly ---"
          : "file must start with a frontmatter block (a line that is exactly ---, then name and description)",
      ),
    ];
  }
  const fields = readSkillFields(lines, block.close);
  const name = fields.find((f) => f.key === "name");
  const description = fields.find((f) => f.key === "description");
  if (!name) {
    out.push(
      finding(file, 1, "frontmatter", "missing required key `name`; add it"),
    );
  } else if (name.value !== dir) {
    out.push(
      finding(
        file,
        name.line,
        "frontmatter",
        `name "${name.value}" must equal the directory name "${dir}"; rename one to match the other`,
      ),
    );
  } else if (!KEBAB_RE.test(name.value)) {
    out.push(
      finding(
        file,
        name.line,
        "frontmatter",
        `name "${name.value}" must be kebab-case (lowercase letters, digits and single dashes)`,
      ),
    );
  }
  if (!description) {
    out.push(
      finding(
        file,
        1,
        "frontmatter",
        "missing required key `description`; add one sentence that says when to use the skill",
      ),
    );
  } else if (description.value === "") {
    out.push(
      finding(
        file,
        description.line,
        "frontmatter",
        "description is empty; say what the skill does and when to use it",
      ),
    );
  } else if (description.value.length > 1024) {
    out.push(
      finding(
        file,
        description.line,
        "frontmatter",
        `description is ${description.value.length} characters; shorten it to at most 1024`,
      ),
    );
  }
  return out;
}

// ---------------------------------------------------------------- feature map

export type Surface = (typeof SURFACES)[number];
export type Reach = (typeof REACH)[number];

export interface Feature {
  id: string;
  title: string;
  summary: string;
  surfaces: string[];
  routes: string[];
  code: string[];
  rize: string[];
  reachMacos: string;
  reachWindows: string;
  reachRize: string;
  verified: string;
  /** Repo-relative path of the feature file. */
  file: string;
  /** Line of each frontmatter key. */
  lines: Record<string, number>;
  /** Whether each `rize` item was double-quoted. */
  rizeQuoted: boolean[];
}

type Value =
  | { kind: "scalar"; value: string }
  | { kind: "list"; items: { text: string; quoted: boolean }[] }
  | { kind: "error"; message: string };

function parseFlatValue(raw: string): Value {
  if (raw.startsWith("[")) {
    if (!raw.endsWith("]"))
      return { kind: "error", message: "list is missing its closing ]" };
    const inner = raw.slice(1, -1).trim();
    if (inner === "") return { kind: "list", items: [] };
    const items: { text: string; quoted: boolean }[] = [];
    let current = "";
    let inQuote = false;
    const push = (): string | null => {
      const piece = current.trim();
      current = "";
      if (piece === "") return "empty list item";
      const quoted = /^".*"$/.test(piece) && piece.length >= 2;
      const text = quoted ? piece.slice(1, -1) : piece;
      if (text.includes('"')) return `stray quote in item ${piece}`;
      items.push({ text, quoted });
      return null;
    };
    for (const ch of inner) {
      if (ch === '"') inQuote = !inQuote;
      if (ch === "," && !inQuote) {
        const problem = push();
        if (problem) return { kind: "error", message: problem };
      } else {
        current += ch;
      }
    }
    if (inQuote)
      return { kind: "error", message: "unbalanced double quote in list" };
    const problem = push();
    if (problem) return { kind: "error", message: problem };
    return { kind: "list", items };
  }
  if (/^[{|>#]/.test(raw))
    return {
      kind: "error",
      message: "only plain scalars and [a, b] lists are allowed",
    };
  return { kind: "scalar", value: raw };
}

const EMPTY_FEATURE = (id: string, file: string): Feature => ({
  id,
  title: "",
  summary: "",
  surfaces: [],
  routes: [],
  code: [],
  rize: [],
  reachMacos: "",
  reachWindows: "",
  reachRize: "",
  verified: "",
  file,
  lines: {},
  rizeQuoted: [],
});

/** `feature-frontmatter`: parses the flat subset and returns the feature with whatever was valid. */
export function ruleFeatureFrontmatter(
  file: string,
  stem: string,
  text: string,
): { findings: Finding[]; feature: Feature; bodyStart: number } {
  const findings: Finding[] = [];
  const add = (line: number, message: string) =>
    findings.push(finding(file, line, "feature-frontmatter", message));
  const feature = EMPTY_FEATURE(stem, file);
  const lines = text.split("\n");
  if (!IDENT_RE.test(stem)) {
    add(
      1,
      `file name "${stem}.md" must be lowercase kebab-case (^[a-z][a-z0-9-]*$); rename it`,
    );
  }
  const block = splitFrontmatter(lines);
  if (!block) {
    add(
      1,
      lines[0] === "---"
        ? "frontmatter block is never closed; end it with a line that is exactly ---"
        : "file must start with a frontmatter block (a line that is exactly ---)",
    );
    return { findings, feature, bodyStart: 0 };
  }
  const seen: { key: string; line: number }[] = [];
  const values = new Map<string, Value>();
  for (let i = 1; i < block.close; i++) {
    const n = i + 1;
    const match = /^([a-z][a-z-]*): (\S.*)$/.exec(lines[i].trimEnd());
    if (!match) {
      add(
        n,
        'each frontmatter line must be "key: value" (no blank lines, comments, nesting or multi-line values)',
      );
      continue;
    }
    const [, key, raw] = match;
    if (values.has(key)) {
      add(n, `duplicate key \`${key}\`; keep one`);
      continue;
    }
    seen.push({ key, line: n });
    feature.lines[key] = n;
    values.set(key, parseFlatValue(raw.trim()));
  }
  const expected = FEATURE_KEYS as readonly string[];
  for (const key of expected) {
    if (!values.has(key)) {
      add(
        block.close + 1,
        `missing required key \`${key}\`; add it (keys, in order: ${expected.join(", ")})`,
      );
    }
  }
  let cursor = 0;
  for (const { key, line } of seen) {
    const index = expected.indexOf(key);
    if (index === -1) {
      add(
        line,
        `unknown key \`${key}\`; allowed keys, in order: ${expected.join(", ")}`,
      );
    } else if (index < cursor) {
      add(
        line,
        `key \`${key}\` is out of order; order is ${expected.join(", ")}`,
      );
    } else {
      cursor = index;
    }
  }

  const scalar = (key: string): string | null => {
    const value = values.get(key);
    if (!value) return null;
    if (value.kind === "scalar") return value.value;
    add(
      feature.lines[key],
      value.kind === "error"
        ? `\`${key}\`: ${value.message}`
        : `\`${key}\` must be a single value, not a list`,
    );
    return null;
  };
  const list = (key: string): { text: string; quoted: boolean }[] | null => {
    const value = values.get(key);
    if (!value) return null;
    if (value.kind === "list") return value.items;
    add(
      feature.lines[key],
      value.kind === "error"
        ? `\`${key}\`: ${value.message}`
        : `\`${key}\` must be a list like [a, b] (use [] for none)`,
    );
    return null;
  };

  const id = scalar("id");
  if (id !== null) {
    feature.id = id;
    if (id !== stem)
      add(
        feature.lines.id,
        `id "${id}" must equal the file name "${stem}"; fix one of them`,
      );
  }
  feature.title = scalar("title") ?? "";
  feature.summary = scalar("summary") ?? "";
  const surfaces = list("surfaces");
  if (surfaces) {
    feature.surfaces = surfaces.map((item) => item.text);
    if (surfaces.length === 0)
      add(
        feature.lines.surfaces,
        `\`surfaces\` must not be empty; use ${SURFACES.join(", ")}`,
      );
    for (const item of surfaces) {
      if (!(SURFACES as readonly string[]).includes(item.text))
        add(
          feature.lines.surfaces,
          `unknown surface "${item.text}"; use ${SURFACES.join(", ")}`,
        );
    }
  }
  const routes = list("routes");
  if (routes) feature.routes = routes.map((item) => item.text);
  const code = list("code");
  if (code) {
    feature.code = code.map((item) => item.text);
    if (code.length === 0)
      add(
        feature.lines.code,
        "`code` must list at least one repo-relative path",
      );
  }
  const rize = list("rize");
  if (rize) {
    feature.rize = rize.map((item) => item.text);
    feature.rizeQuoted = rize.map((item) => item.quoted);
    for (const item of rize) {
      if (!item.quoted)
        add(
          feature.lines.rize,
          `rize item ${item.text} must be a double-quoted string like "rize entries list"`,
        );
    }
  }
  for (const [key, field] of [
    ["reach-macos", "reachMacos"],
    ["reach-windows", "reachWindows"],
    ["reach-rize", "reachRize"],
  ] as const) {
    const value = scalar(key);
    if (value === null) continue;
    feature[field] = value;
    if (!(REACH as readonly string[]).includes(value))
      add(
        feature.lines[key],
        `\`${key}\` is "${value}"; use ${REACH.join(", ")}`,
      );
  }
  const verified = scalar("verified");
  if (verified !== null) {
    feature.verified = verified;
    if (!VERIFIED_RE.test(verified))
      add(
        feature.lines.verified,
        `verified "${verified}" must be \`never\` or "YYYY-MM-DD <macos-drive|windows-drive|rize>"`,
      );
  }
  return { findings, feature, bodyStart: block.close + 1 };
}

/** `feature-body`: one H1 equal to the title, then the six H2 sections in order, each non-empty. */
export function ruleFeatureBody(
  file: string,
  title: string,
  text: string,
  bodyStart: number,
): Finding[] {
  const out: Finding[] = [];
  const add = (line: number, message: string) =>
    out.push(finding(file, line, "feature-body", message));
  const body = scanLines(text)
    .slice(bodyStart)
    .filter((line) => line.kind === "text");
  const headings = body.flatMap((line) => {
    const match = /^(#{1,6}) +(.+?)\s*$/.exec(line.text);
    return match ? [{ n: line.n, level: match[1].length, text: match[2] }] : [];
  });
  const h1s = headings.filter((h) => h.level === 1);
  if (h1s.length === 0) {
    add(1, `missing the single H1; add "# ${title}" after the frontmatter`);
  } else {
    if (h1s.length > 1) {
      for (const extra of h1s.slice(1))
        add(extra.n, "only one H1 is allowed; make this an H2 or remove it");
    }
    if (title && h1s[0].text !== title)
      add(
        h1s[0].n,
        `H1 must be exactly "# ${title}" (the frontmatter title); it is "# ${h1s[0].text}"`,
      );
  }
  const h2s = headings.filter((h) => h.level === 2);
  const expected = FEATURE_SECTIONS as readonly string[];
  let cursor = -1;
  const found = new Set<string>();
  for (const h2 of h2s) {
    const index = expected.indexOf(h2.text);
    if (index === -1) {
      add(
        h2.n,
        `unexpected section "## ${h2.text}"; the sections are ${expected.map((s) => `## ${s}`).join(", ")}`,
      );
    } else if (found.has(h2.text)) {
      add(h2.n, `section "## ${h2.text}" appears twice; keep one`);
    } else if (index < cursor) {
      add(
        h2.n,
        `section "## ${h2.text}" is out of order; order is ${expected.join(", ")}`,
      );
    } else {
      cursor = index;
    }
    found.add(h2.text);
  }
  for (const section of expected) {
    if (!found.has(section))
      add(1, `missing section "## ${section}"; add it with real content`);
  }
  const lines = text.split("\n");
  h2s.forEach((h2, index) => {
    if (!expected.includes(h2.text)) return;
    const next = h2s[index + 1]?.n ?? lines.length + 1;
    const content = lines
      .slice(h2.n, next - 1)
      .join("\n")
      .trim();
    if (content === "")
      add(h2.n, `section "## ${h2.text}" is empty; write its content`);
  });
  return out;
}

/** `feature-code`: every listed path exists. */
export function ruleFeatureCode(root: string, feature: Feature): Finding[] {
  const out: Finding[] = [];
  for (const path of feature.code) {
    const bad = path.startsWith("/") || path.split("/").includes("..");
    if (bad || !existsSync(join(root, path))) {
      out.push(
        finding(
          feature.file,
          feature.lines.code ?? 1,
          "feature-code",
          bad
            ? `code path "${path}" must be repo-relative; fix it`
            : `code path "${path}" does not exist; fix the path or remove it`,
        ),
      );
    }
  }
  return out;
}

/** The `name: "<x>"` entries of `export type Route =`, or null when the anchor is missing. */
export function parseRoutes(
  typesText: string,
): { name: string; line: number }[] | null {
  const anchor = typesText.indexOf("export type Route =");
  if (anchor === -1) return null;
  const blank = (match: string) => match.replace(/[^\n]/g, " ");
  const clean = typesText
    .replace(/\/\*[\s\S]*?\*\//g, blank)
    .replace(/\/\/[^\n]*/g, blank);
  const depth: number[] = [];
  let level = 0;
  let end = clean.length;
  for (let i = anchor; i < clean.length; i++) {
    const ch = clean[i];
    if (ch === "{") level++;
    else if (ch === "}") level--;
    depth[i] = level;
    if (ch === ";" && level === 0) {
      end = i;
      break;
    }
  }
  const slice = clean.slice(anchor, end);
  const routes: { name: string; line: number }[] = [];
  for (const match of slice.matchAll(/\bname:\s*"([^"]+)"/g)) {
    const at = anchor + (match.index ?? 0);
    if (depth[at] !== 1) continue;
    routes.push({
      name: match[1],
      line: clean.slice(0, at).split("\n").length,
    });
  }
  return routes;
}

/** `feature-route`: routes are real, and every route has exactly one owner. */
export function ruleFeatureRoute(root: string, features: Feature[]): Finding[] {
  const typesText = readText(root, TYPES_TS);
  const routes = typesText === null ? null : parseRoutes(typesText);
  if (!routes) {
    return [
      finding(
        TYPES_TS,
        1,
        "feature-route",
        "could not find the Route union; update the linter",
      ),
    ];
  }
  const out: Finding[] = [];
  const known = new Set(routes.map((r) => r.name));
  const owners = new Map<string, Feature[]>();
  for (const feature of features) {
    for (const name of feature.routes) {
      if (!known.has(name)) {
        out.push(
          finding(
            feature.file,
            feature.lines.routes ?? 1,
            "feature-route",
            `route "${name}" is not a Route in ${TYPES_TS} (${[...known].join(", ")}); fix the name`,
          ),
        );
      }
      owners.set(name, [...(owners.get(name) ?? []), feature]);
    }
  }
  for (const route of routes) {
    const list = owners.get(route.name) ?? [];
    if (list.length === 0) {
      out.push(
        finding(
          TYPES_TS,
          route.line,
          "feature-route",
          `route "${route.name}" is owned by no feature; list it in the routes of exactly one file in ${FEATURES_DIR}/`,
        ),
      );
    }
    if (list.length > 1) {
      for (const feature of list) {
        out.push(
          finding(
            feature.file,
            feature.lines.routes ?? 1,
            "feature-route",
            `route "${route.name}" is owned by ${list.length} features (${list.map((f) => f.id).join(", ")}); keep it in exactly one`,
          ),
        );
      }
    }
  }
  return out;
}

/** `feature-coverage`: every page and window entry point is covered by some feature's `code`. */
export function ruleFeatureCoverage(
  root: string,
  features: Feature[],
): Finding[] {
  const targets = [
    ...listDir(root, "src/pages")
      .filter(
        (name) =>
          name.endsWith(".tsx") && !isDirectory(root, `src/pages/${name}`),
      )
      .map((name) => `src/pages/${name}`),
    "src/Pulse.tsx",
    "src/Reminder.tsx",
  ].filter((path) => existsSync(join(root, path)));
  const covers = (entry: string, path: string) => {
    const clean = posix.normalize(entry).replace(/\/$/, "");
    return clean === path || path.startsWith(`${clean}/`);
  };
  const out: Finding[] = [];
  for (const path of targets) {
    if (!features.some((f) => f.code.some((entry) => covers(entry, path)))) {
      out.push(
        finding(
          path,
          1,
          "feature-coverage",
          `not covered by any feature's \`code\` in ${FEATURES_DIR}/; add it (or a directory containing it) to the feature that owns it`,
        ),
      );
    }
  }
  return out;
}

/** `feature-rize`: every `rize` item is a Shipped command, written as command words only. */
export function ruleFeatureRize(catalog: Catalog, feature: Feature): Finding[] {
  const out: Finding[] = [];
  const line = feature.lines.rize ?? 1;
  const add = (message: string) =>
    out.push(finding(feature.file, line, "feature-rize", message));
  for (const item of feature.rize) {
    const words = item.trim().split(/\s+/);
    if (words[0] !== "rize") {
      add(`"${item}" must start with "rize"`);
      continue;
    }
    const rest = words.slice(1);
    if (rest.length > 2 || rest.some((w) => !IDENT_RE.test(w))) {
      add(
        `"${item}" must be command words only (at most a command and a subcommand, no arguments or flags)`,
      );
      continue;
    }
    const problem = rizeProblem(rest, catalog);
    if (problem) {
      add(`"${item}": ${problem}`);
    } else if (
      rest.length === 1 &&
      (catalog.rize.get(rest[0])?.size ?? 0) > 0
    ) {
      add(
        `"${item}" needs a subcommand (${[...(catalog.rize.get(rest[0]) ?? [])].join(", ")})`,
      );
    }
  }
  return out;
}

interface IndexRow {
  n: number;
  text: string;
  target: string;
}

/** `feature-index`: README.md lists every feature file exactly once and nothing else. */
export function ruleFeatureIndex(
  root: string,
  stems: string[],
  titles: Map<string, string>,
): Finding[] {
  const file = `${FEATURES_DIR}/README.md`;
  const text = readText(root, file);
  if (text === null) {
    return [
      finding(
        file,
        1,
        "feature-index",
        "README.md is missing; add the index (# Feature map, then the feature table)",
      ),
    ];
  }
  const out: Finding[] = [];
  const add = (line: number, message: string) =>
    out.push(finding(file, line, "feature-index", message));
  const scanned = scanLines(text).filter((line) => line.kind === "text");
  const h1 = scanned.find((line) => /^# /.test(line.text));
  if (!h1) add(1, "missing the H1 `# Feature map`");
  else if (h1.text.trim() !== "# Feature map")
    add(h1.n, `H1 must be exactly "# Feature map"; it is "${h1.text.trim()}"`);

  const headerAt = scanned.findIndex(
    (line) => line.text.trim() === INDEX_HEADER,
  );
  const rows: IndexRow[] = [];
  if (headerAt === -1) {
    add(1, `missing the table header ${INDEX_HEADER}`);
  } else {
    for (const line of scanned.slice(headerAt + 1)) {
      if (!line.text.startsWith("|")) break;
      if (/^\|[\s:|-]+\|$/.test(line.text.trim())) continue;
      const cells = splitCells(line.text);
      if (cells.length !== 5) {
        add(
          line.n,
          `table row has ${cells.length} cells; the table has 5 columns`,
        );
        continue;
      }
      const link = /^\[([^\]]+)\]\(([^)\s]+)\)$/.exec(cells[0]);
      if (!link) {
        add(line.n, "first cell must be a relative link like [Title](id.md)");
        continue;
      }
      rows.push({ n: line.n, text: link[1], target: link[2] });
    }
  }
  const linked = new Map<string, number>();
  for (const row of rows) {
    const stem = row.target.replace(/\.md$/, "");
    const isFeature = row.target.endsWith(".md") && stems.includes(stem);
    if (!isFeature) {
      add(
        row.n,
        existsSync(join(root, FEATURES_DIR, row.target))
          ? `"${row.target}" is not a feature file; the table lists feature files only`
          : `links "${row.target}", which does not exist; fix the link or add the file`,
      );
      continue;
    }
    linked.set(stem, (linked.get(stem) ?? 0) + 1);
    if (linked.get(stem) === 2)
      add(row.n, `${row.target} is linked more than once; keep one row`);
    const title = titles.get(stem);
    if (title && row.text !== title)
      add(
        row.n,
        `link text "${row.text}" must equal the feature title "${title}"`,
      );
  }
  const anchor = headerAt === -1 ? 1 : scanned[headerAt].n;
  if (headerAt !== -1) {
    for (const stem of stems) {
      if (!linked.has(stem))
        add(
          anchor,
          `${stem}.md is not linked from the table; add a row [${titles.get(stem) || stem}](${stem}.md)`,
        );
    }
  }
  const h2s = scanned.flatMap((line, index) =>
    /^## /.test(line.text) ? [{ line, index }] : [],
  );
  const section = h2s.find(
    (h) => h.line.text.trim() === `## ${INDEX_NOT_REACHABLE}`,
  );
  if (!section) {
    add(1, `missing the section "## ${INDEX_NOT_REACHABLE}"`);
  } else {
    const next = h2s.find((h) => h.index > section.index);
    const content = scanned
      .slice(section.index + 1, next?.index)
      .map((line) => line.text)
      .join("\n")
      .trim();
    if (content === "")
      add(section.line.n, `section "## ${INDEX_NOT_REACHABLE}" is empty`);
  }
  return out;
}

// ---------------------------------------------------------------- loading and orchestration

interface FeatureFile {
  stem: string;
  file: string;
  text: string;
}

function readFeatureFiles(root: string): FeatureFile[] | null {
  if (!isDirectory(root, FEATURES_DIR)) return null;
  return listDir(root, FEATURES_DIR)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .flatMap((name) => {
      const file = `${FEATURES_DIR}/${name}`;
      const text = readText(root, file);
      return text === null ? [] : [{ stem: name.slice(0, -3), file, text }];
    });
}

/** Features in README table order; any not linked there follow, sorted by id. */
export function loadFeatures(root: string): Feature[] {
  const files = readFeatureFiles(root);
  if (!files) return [];
  const features = files.map(
    (f) => ruleFeatureFrontmatter(f.file, f.stem, f.text).feature,
  );
  const index = readText(root, `${FEATURES_DIR}/README.md`) ?? "";
  const order: string[] = [];
  for (const match of index.matchAll(/^\|\s*\[[^\]]+\]\(([^)\s]+)\.md\)/gm))
    order.push(match[1]);
  const rank = (id: string) => {
    const at = order.indexOf(id);
    return at === -1 ? order.length : at;
  };
  return features.sort((a, b) => rank(a.id) - rank(b.id));
}

function sortFindings(findings: Finding[]): Finding[] {
  const seen = new Set<string>();
  return findings
    .filter((f) => {
      const key = `${f.file}\0${f.line}\0${f.rule}\0${f.message}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort(
      (a, b) =>
        a.file.localeCompare(b.file) ||
        a.line - b.line ||
        a.rule.localeCompare(b.rule),
    );
}

export function lintRepo(root: string): Finding[] {
  const catalog = loadCatalog(root);
  const findings: Finding[] = [];

  for (const dir of listSkills(root)) {
    const file = `${SKILLS_DIR}/${dir}/SKILL.md`;
    const text = readText(root, file);
    if (text !== null) findings.push(...ruleSkillFrontmatter(file, dir, text));
  }
  for (const file of markdownFiles(root, SKILLS_DIR)) {
    const text = readText(root, file);
    if (text === null) continue;
    findings.push(
      ...ruleLink(root, file, text),
      ...(PATH_RULE_SCOPES.some((scope) => file.startsWith(scope))
        ? rulePath(root, file, text)
        : []),
      ...rulePlaceholder(file, text),
      ...ruleCommand(catalog, file, text),
    );
  }

  const files = readFeatureFiles(root);
  if (files) {
    const features: Feature[] = [];
    const titles = new Map<string, string>();
    for (const f of files) {
      const parsed = ruleFeatureFrontmatter(f.file, f.stem, f.text);
      findings.push(
        ...parsed.findings,
        ...ruleFeatureBody(
          f.file,
          parsed.feature.title,
          f.text,
          parsed.bodyStart,
        ),
        ...ruleFeatureCode(root, parsed.feature),
        ...ruleFeatureRize(catalog, parsed.feature),
      );
      features.push(parsed.feature);
      titles.set(f.stem, parsed.feature.title);
    }
    findings.push(
      ...ruleFeatureRoute(root, features),
      ...ruleFeatureCoverage(root, features),
      ...ruleFeatureIndex(
        root,
        files.map((f) => f.stem),
        titles,
      ),
    );
  }
  return sortFindings(findings);
}

/** True for findings that mean the feature map itself is broken (used to gate `coverage`). */
export function isMapFinding(f: Finding): boolean {
  return f.rule.startsWith("feature-") || f.file.startsWith(`${FEATURES_DIR}/`);
}

/** The per-feature coverage table template for a maintenance pass, in index order. */
export function coverageTable(root: string): string {
  const rows = loadFeatures(root).map(
    (f) =>
      `| ${f.title} | ${f.surfaces.join(", ")} | ${f.reachMacos} / ${f.reachWindows} / ${f.reachRize} | | | |`,
  );
  return [
    "| Feature | Surfaces | Reach (macOS / Windows / rize) | Result | Evidence | Notes |",
    "| --- | --- | --- | --- | --- | --- |",
    ...rows,
  ].join("\n");
}
