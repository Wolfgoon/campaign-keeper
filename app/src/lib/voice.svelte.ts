// Reads the GM's replies aloud, each character in their own voice. The GM casts them; the player
// can change any of them. The cast is kept per computer, since voices differ between computers.

const KEY = "ck-cast";

function loadCast(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "{}");
  } catch {
    return {};
  }
}

export const cast = $state<Record<string, string>>(loadCast());

export function setCast(name: string, voice: string) {
  cast[name] = voice;
  try {
    localStorage.setItem(KEY, JSON.stringify(cast));
  } catch {
    /* the choice just lasts this run */
  }
}

export function voices(): SpeechSynthesisVoice[] {
  return typeof speechSynthesis === "undefined" ? [] : speechSynthesis.getVoices();
}

const FEMALE = /zira|aria|jenny|hazel|susan|samantha|victoria|karen|female|michelle|emma|ava/i;
const MALE = /david|mark|guy|george|daniel|alex|fred|male|ryan|brian|andrew/i;

/** A stable pick: the same name gets the same voice each time. */
function pick(name: string, gender: string | undefined, wanted?: string): string | undefined {
  const all = voices().filter((v) => v.lang.startsWith("en"));
  if (wanted && voices().some((v) => v.name.includes(wanted))) return voices().find((v) => v.name.includes(wanted))!.name;
  const pool = all.filter((v) => (gender === "female" ? FEMALE : gender === "male" ? MALE : /./).test(v.name));
  const list = pool.length ? pool : all;
  if (!list.length) return undefined;
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return list[h % list.length].name;
}

const PITCH: Record<string, number> = { "very low": 0.5, low: 0.75, normal: 1, high: 1.25, "very high": 1.5 };
const PACE: Record<string, number> = { "very slow": 0.7, slow: 0.85, normal: 1, fast: 1.15, "very fast": 1.3 };

function say(text: string, voice: string | undefined, pitch: number, rate: number) {
  const clean = text.replace(/[*_#`]/g, "").trim();
  if (!clean) return;
  const u = new SpeechSynthesisUtterance(clean);
  const v = voices().find((x) => x.name === voice);
  if (v) u.voice = v;
  u.pitch = pitch;
  u.rate = rate;
  speechSynthesis.speak(u);
}

export function stopSpeaking() {
  if (typeof speechSynthesis !== "undefined") speechSynthesis.cancel();
}

/** Speak one reply: narration in the narrator's voice, tagged lines in the characters'. */
export function speakEntry(text: string, tagged: any[] | undefined, speed: number) {
  if (typeof speechSynthesis === "undefined") return;
  stopSpeaking();
  const narrator = cast["Narrator"] ?? pick("Narrator", undefined);
  let rest = text;
  for (const t of tagged ?? []) {
    const line: string = t.text ?? "";
    const i = rest.indexOf(line);
    if (i < 0) continue;
    say(rest.slice(0, i), narrator, 1, speed);
    const name: string = t.name ?? "Someone";
    const a = t.attrs ?? {};
    if (!cast[name]) {
      const v = pick(name, a.gender, a.voice);
      if (v) setCast(name, v);
    }
    say(line, cast[name], PITCH[a.pitch] ?? (Number(a.pitch) ? 1 + Number(a.pitch) / 2 : 1), (PACE[a.pace] ?? 1) * speed);
    rest = rest.slice(i + line.length);
  }
  say(rest, narrator, 1, speed);
}
