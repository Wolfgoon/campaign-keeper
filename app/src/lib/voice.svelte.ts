// Reads the GM's replies aloud with a speech engine that ships inside the app (eSpeak), so it
// sounds the same on every computer and phone and never depends on the voices a system has.
// The narrator is one flat, even, neither-male-nor-female voice. Characters get one of ten fixed
// voices, five male and five female; the GM casts them by name and the player can change any.
import meSpeak from "mespeak";
import config from "mespeak/src/mespeak_config.json";
import enUS from "mespeak/voices/en/en-us.json";

meSpeak.loadConfig(config);
meSpeak.loadVoice(enUS);

/** The ten character voices. `id` is what the GM writes in voice="…". */
export const VOICES = [
  { id: "Abel", gender: "male", about: "deep, steady", variant: "m4", pitch: 30 },
  { id: "Bram", gender: "male", about: "rough, gravelly", variant: "m1", pitch: 38 },
  { id: "Cass", gender: "male", about: "plain, even", variant: "m3", pitch: 45 },
  { id: "Dov", gender: "male", about: "warm, echoing", variant: "m2", pitch: 50 },
  { id: "Edge", gender: "male", about: "light, quick", variant: "m6", pitch: 58 },
  { id: "Fay", gender: "female", about: "low, calm", variant: "f4", pitch: 40 },
  { id: "Gwen", gender: "female", about: "clear, bright", variant: "f1", pitch: 50 },
  { id: "Hana", gender: "female", about: "soft, breathy", variant: "f2", pitch: 55 },
  { id: "Isla", gender: "female", about: "crisp, firm", variant: "f3", pitch: 48 },
  { id: "Juno", gender: "female", about: "high, airy", variant: "f5", pitch: 65 },
] as const;

const NARRATOR = { variant: "", pitch: 55 };
const KEY = "ck-cast2";

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

/** A stable pick: the GM's choice if it names one of ours, otherwise the same name gets the same voice. */
function pick(name: string, gender: string | undefined, wanted?: string): string {
  const w = wanted && VOICES.find((v) => v.id.toLowerCase() === String(wanted).toLowerCase());
  if (w) return w.id;
  const pool = VOICES.filter((v) => gender !== "male" && gender !== "female" ? true : v.gender === gender);
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return pool[h % pool.length].id;
}

const PITCH: Record<string, number> = { "very low": -20, low: -10, normal: 0, high: 10, "very high": 20 };
const PACE: Record<string, number> = { "very slow": 0.7, slow: 0.85, normal: 1, fast: 1.15, "very fast": 1.3 };

// One queue of clips played back to back, so stop really stops.
let ctx: AudioContext | undefined;
let queue: Promise<void> = Promise.resolve();
let playing: AudioBufferSourceNode | undefined;
let run = 0;

function say(text: string, variant: string, pitch: number, rate: number, flat = false) {
  const clean = text.replace(/[*_#`<>]/g, "").replace(/\s+/g, " ").trim();
  if (!clean) return;
  const me = run;
  // The narrator reads on one level: no rise and fall.
  const body = flat ? `<speak><prosody range="x-low">${clean.replace(/&/g, "&amp;")}</prosody></speak>` : clean;
  const wav: ArrayBuffer | null = meSpeak.speak(body, {
    ssml: flat,
    rawdata: "buffer",
    variant: variant || undefined,
    pitch: Math.max(0, Math.min(99, Math.round(pitch))),
    speed: Math.round(165 * rate),
    wordgap: 1,
  });
  if (!wav) return;
  ctx ??= new AudioContext();
  const c = ctx;
  const decoded = c.decodeAudioData(wav.slice(0));
  queue = queue.then(async () => {
    if (me !== run) return;
    const buf = await decoded.catch(() => null);
    if (!buf || me !== run) return;
    await new Promise<void>((done) => {
      const s = c.createBufferSource();
      s.buffer = buf;
      s.connect(c.destination);
      s.onended = () => done();
      playing = s;
      s.start();
    });
  });
}

export function stopSpeaking() {
  run++;
  try {
    playing?.stop();
  } catch {
    /* already stopped */
  }
  queue = Promise.resolve();
}

/** Try a voice from the Voices panel. */
export function sample(id: string) {
  stopSpeaking();
  const v = VOICES.find((x) => x.id === id);
  if (v) say(`I am ${v.id}.`, v.variant, v.pitch, 1);
}

/** Speak one reply: narration in the narrator's voice, tagged lines in the characters'. */
export function speakEntry(text: string, tagged: any[] | undefined, speed: number) {
  stopSpeaking();
  ctx?.resume();
  let rest = text;
  for (const t of tagged ?? []) {
    const line: string = t.text ?? "";
    const i = rest.indexOf(line);
    if (i < 0) continue;
    say(rest.slice(0, i), NARRATOR.variant, NARRATOR.pitch, speed, true);
    const name: string = t.name ?? "Someone";
    const a = t.attrs ?? {};
    if (!cast[name] || !VOICES.some((v) => v.id === cast[name])) setCast(name, pick(name, a.gender, a.voice));
    const v = VOICES.find((x) => x.id === cast[name])!;
    const shift = PITCH[a.pitch] ?? (Number(a.pitch) ? Number(a.pitch) * 20 : 0);
    say(line, v.variant, v.pitch + shift, (PACE[a.pace] ?? 1) * speed);
    rest = rest.slice(i + line.length);
  }
  say(rest, NARRATOR.variant, NARRATOR.pitch, speed, true);
}
