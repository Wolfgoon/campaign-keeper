import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { marked } from "marked";
import DOMPurify from "dompurify";

export const call = invoke as <T = any>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

export function onGame(f: (kind: string, data: any) => void) {
  return listen<{ kind: string; data: any }>("game", (e) => f(e.payload.kind, e.payload.data));
}
export function onTable(f: () => void) {
  return listen("table", () => f());
}
export function onClient(f: (msg: any) => void) {
  return listen<any>("client", (e) => f(e.payload));
}

marked.setOptions({ gfm: true, breaks: true });

/** The GM's text is model output: always sanitized before it reaches the page. */
export function md(text: string): string {
  return DOMPurify.sanitize(marked.parse(text ?? "", { async: false }) as string);
}

/** Pictures are SVG written by a model. No scripts, no external references. */
export function safeSvg(svg: string): string {
  return DOMPurify.sanitize(svg ?? "", {
    USE_PROFILES: { svg: true, svgFilters: true },
    FORBID_TAGS: ["script", "foreignObject", "image", "use", "a"],
    FORBID_ATTR: ["href", "xlink:href"],
  });
}

let toastSetter: ((s: string) => void) | null = null;
export function bindToast(f: (s: string) => void) {
  toastSetter = f;
}
export function toast(s: unknown) {
  toastSetter?.(String(s));
}

export const MODELS = [
  { id: "claude-opus-5-5", name: "Claude Opus 5.5", note: "Recommended: strong storytelling and continuity. $4 / $20 per million tokens in / out." },
  { id: "claude-sonnet-5-5", name: "Claude Sonnet 5.5", note: "Faster and cheaper. $2 / $10 per million tokens in / out." },
  { id: "claude-haiku-5-5", name: "Claude Haiku 5.5", note: "Fastest and cheapest, lighter on continuity. $0.10 / $0.50 per million tokens." },
  { id: "claude-fable-5-1", name: "Claude Fable 5.1", note: "Anthropic's most capable model. $10 / $50 per million tokens in / out." },
];
