// The GM's scores, played with WebAudio. Each token is one eighth-note step: a note (C4, F#3),
// a chord (Dm7, C/E), notes joined with +, - to hold, . for silence. Drums: k s h o t c.

const SEMI: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
const QUALITY: [RegExp, number[]][] = [
  [/^maj7/, [0, 4, 7, 11]], [/^m7b5/, [0, 3, 6, 10]], [/^m7/, [0, 3, 7, 10]], [/^m9/, [0, 3, 7, 10, 14]], [/^min/, [0, 3, 7]], [/^m/, [0, 3, 7]],
  [/^dim7?/, [0, 3, 6]], [/^aug/, [0, 4, 8]], [/^sus2/, [0, 2, 7]], [/^sus4?/, [0, 5, 7]], [/^add9/, [0, 4, 7, 14]], [/^7/, [0, 4, 7, 10]], [/^9/, [0, 4, 7, 10, 14]], [/^6/, [0, 4, 7, 9]], [/^$/, [0, 4, 7]],
];

function hz(midi: number) {
  return 440 * Math.pow(2, (midi - 69) / 12);
}

export function parseToken(tok: string): number[] {
  const out: number[] = [];
  for (const part of tok.split("+")) {
    const m = part.match(/^([A-G])([#b]?)(.*)$/);
    if (!m) continue;
    const pc = SEMI[m[1]] + (m[2] === "#" ? 1 : m[2] === "b" ? -1 : 0);
    const note = m[3].match(/^(-?\d+)$/);
    if (note) {
      out.push(pc + (parseInt(note[1]) + 1) * 12);
      continue;
    }
    const [qual, bass] = m[3].split("/");
    const q = QUALITY.find(([r]) => r.test(qual));
    const root = 48 + pc;
    for (const i of q ? q[1] : [0, 4, 7]) out.push(root + i);
    const b = bass?.match(/^([A-G])([#b]?)/);
    if (b) out.push(36 + SEMI[b[1]] + (b[2] === "#" ? 1 : b[2] === "b" ? -1 : 0));
  }
  return out;
}

interface Line { voice: string; steps: string[] }

export function parseScore(score: string): { lines: Line[]; length: number } {
  const lines: Line[] = [];
  for (const raw of score.split("\n")) {
    const i = raw.indexOf(":");
    if (i < 0) continue;
    const steps = raw.slice(i + 1).replace(/\|/g, " ").trim().split(/\s+/).filter(Boolean);
    if (steps.length) lines.push({ voice: raw.slice(0, i).trim().toLowerCase(), steps });
  }
  return { lines, length: Math.max(0, ...lines.map((l) => l.steps.length)) };
}

const VOICES: Record<string, { type: OscillatorType; attack: number; release: number; gain: number; shift?: number }> = {
  pad: { type: "sawtooth", attack: 0.5, release: 0.9, gain: 0.05 },
  strings: { type: "sawtooth", attack: 0.25, release: 0.5, gain: 0.06 },
  keys: { type: "triangle", attack: 0.01, release: 0.5, gain: 0.13 },
  pluck: { type: "triangle", attack: 0.003, release: 0.3, gain: 0.16 },
  bell: { type: "sine", attack: 0.003, release: 1.3, gain: 0.12, shift: 12 },
  bass: { type: "triangle", attack: 0.01, release: 0.2, gain: 0.24 },
  lead: { type: "square", attack: 0.02, release: 0.2, gain: 0.05 },
  flute: { type: "sine", attack: 0.07, release: 0.2, gain: 0.15 },
};

class Player {
  ctx: AudioContext | null = null;
  master: GainNode | null = null;
  timer: number | null = null;
  title = $state("");
  volume = 0.5;
  muted = false;

  private ensure() {
    if (!this.ctx) {
      this.ctx = new AudioContext();
      this.master = this.ctx.createGain();
      const lp = this.ctx.createBiquadFilter();
      lp.type = "lowpass";
      lp.frequency.value = 3200;
      this.master.connect(lp).connect(this.ctx.destination);
    }
    this.setVolume(this.volume, this.muted);
    return this.ctx;
  }

  setVolume(v: number, muted: boolean) {
    this.volume = v;
    this.muted = muted;
    if (this.master) this.master.gain.value = muted ? 0 : v;
  }

  stop() {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    this.title = "";
  }

  private tone(time: number, dur: number, midi: number, name: string) {
    const ctx = this.ctx!;
    const v = VOICES[name] ?? VOICES.keys;
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.type = v.type;
    o.frequency.value = hz(midi + (v.shift ?? 0));
    g.gain.setValueAtTime(0, time);
    g.gain.linearRampToValueAtTime(v.gain, time + v.attack);
    g.gain.setValueAtTime(v.gain, time + Math.max(v.attack, dur));
    g.gain.exponentialRampToValueAtTime(0.0008, time + dur + v.release);
    o.connect(g).connect(this.master!);
    o.start(time);
    o.stop(time + dur + v.release + 0.02);
  }

  private noise(time: number, dur: number, gain: number, freq: number) {
    const ctx = this.ctx!;
    const buf = ctx.createBuffer(1, Math.max(1, Math.floor(ctx.sampleRate * dur)), ctx.sampleRate);
    const d = buf.getChannelData(0);
    // A fixed pattern of noise: drum colour, nothing to do with the game.
    let x = 1;
    for (let i = 0; i < d.length; i++) {
      x = (x * 16807) % 2147483647;
      d[i] = x / 1073741823.5 - 1;
    }
    const s = ctx.createBufferSource();
    s.buffer = buf;
    const f = ctx.createBiquadFilter();
    f.type = "highpass";
    f.frequency.value = freq;
    const g = ctx.createGain();
    g.gain.setValueAtTime(gain, time);
    g.gain.exponentialRampToValueAtTime(0.001, time + dur);
    s.connect(f).connect(g).connect(this.master!);
    s.start(time);
  }

  private drum(time: number, tok: string) {
    const ctx = this.ctx!;
    for (const c of tok) {
      if (c === "k" || c === "t") {
        const o = ctx.createOscillator();
        const g = ctx.createGain();
        const f0 = c === "k" ? 120 : 200;
        o.frequency.setValueAtTime(f0, time);
        o.frequency.exponentialRampToValueAtTime(f0 / 3, time + 0.12);
        g.gain.setValueAtTime(0.4, time);
        g.gain.exponentialRampToValueAtTime(0.001, time + 0.18);
        o.connect(g).connect(this.master!);
        o.start(time);
        o.stop(time + 0.2);
      } else if (c === "s") this.noise(time, 0.14, 0.2, 1500);
      else if (c === "h") this.noise(time, 0.04, 0.08, 7000);
      else if (c === "o") this.noise(time, 0.22, 0.08, 6000);
      else if (c === "c") this.noise(time, 0.8, 0.14, 4000);
    }
  }

  play(track: { title?: string; tempo?: string | number; score?: string }) {
    const { lines, length } = parseScore(track.score ?? "");
    if (!length) return;
    this.stop();
    const ctx = this.ensure();
    void ctx.resume();
    const stepDur = 60 / Math.max(30, Number(track.tempo) || 90) / 2;
    let step = 0;
    let next = ctx.currentTime + 0.1;
    const tick = () => {
      while (next < ctx.currentTime + 0.4) {
        for (const l of lines) {
          const tok = l.steps[step % l.steps.length];
          if (tok === "-" || tok === ".") continue;
          if (l.voice === "drums") {
            this.drum(next, tok);
            continue;
          }
          let hold = 1;
          while (l.steps[(step + hold) % l.steps.length] === "-" && hold < l.steps.length) hold++;
          for (const n of parseToken(tok)) this.tone(next, hold * stepDur, n, l.voice);
        }
        step = (step + 1) % length;
        next += stepDur;
      }
    };
    tick();
    this.timer = setInterval(tick, 120) as unknown as number;
    this.title = track.title ?? "Untitled";
  }
}

export const music = new Player();
