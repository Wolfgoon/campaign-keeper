import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { webCall, onClient as onWebClient } from "./web";

/** False when the page is open in a plain web browser: a friend joining a table without the app. */
export const inApp = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const call = (inApp ? invoke : webCall) as <T = any>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

const nothing = () => Promise.resolve(() => {});

export function onGame(f: (kind: string, data: any) => void) {
  return inApp ? listen<{ kind: string; data: any }>("game", (e) => f(e.payload.kind, e.payload.data)) : nothing();
}
export function onTable(f: () => void) {
  return inApp ? listen("table", () => f()) : nothing();
}
export function onClient(f: (msg: any) => void) {
  return inApp ? listen<any>("client", (e) => f(e.payload)) : Promise.resolve(onWebClient(f));
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

export const AGY_MODELS = [
  { id: "", name: "Default (Gemini 4 Argon Medium)", note: "Antigravity's default model and generous quota." },
  { id: "claude-opus-5-5-medium", name: "Claude Opus 5.5 (Medium)", note: "Strongest storytelling and continuity. Uses your Antigravity Claude quota." },
  { id: "claude-opus-5-5-high", name: "Claude Opus 5.5 (High)", note: "Claude Opus 5.5 with deeper thinking." },
  { id: "claude-opus-5-5-low", name: "Claude Opus 5.5 (Low)", note: "Claude Opus 5.5 with quicker replies." },
  { id: "claude-sonnet-5-5-medium", name: "Claude Sonnet 5.5 (Medium)", note: "Fast and capable Claude model." },
  { id: "claude-sonnet-5-5-high", name: "Claude Sonnet 5.5 (High)", note: "Claude Sonnet 5.5 with deeper thinking." },
  { id: "claude-sonnet-5-5-low", name: "Claude Sonnet 5.5 (Low)", note: "Claude Sonnet 5.5 with quickest replies." },
  { id: "gemini-4-argon-medium", name: "Gemini 4 Argon (Medium)", note: "Flagship Gemini model with balanced thinking." },
  { id: "gemini-4-argon-high", name: "Gemini 4 Argon (High)", note: "Gemini 4 Argon with deeper thinking." },
  { id: "gemini-4-argon-low", name: "Gemini 4 Argon (Low)", note: "Gemini 4 Argon with quicker replies." },
  { id: "gemini-3.8-flash-medium", name: "Gemini 3.8 Flash (Medium)", note: "Fast, lightweight Gemini model." },
  { id: "gemini-3.8-flash-high", name: "Gemini 3.8 Flash (High)", note: "Gemini 3.8 Flash with deeper thinking." },
  { id: "gemini-3.8-flash-low", name: "Gemini 3.8 Flash (Low)", note: "Fastest Gemini replies." },
  { id: "gpt-oss-120b-medium", name: "GPT-OSS 120B (Medium)", note: "Open-weights 120B model hosted on Antigravity." },
];
