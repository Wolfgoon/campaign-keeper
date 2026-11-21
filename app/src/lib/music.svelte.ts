// The GM's scores, played with WebAudio. Each token is one eighth-note step: a note (C4, F#3),
// a chord (Dm7, C/E), notes joined with +, - to hold, . for silence. Drums: k s h o t c.
// Scores can have [sections] and a form, each voice a level and pan, and tracks crossfade.
// Ambience is place sound underneath. All noise comes from a fixed formula, not a random source.

const SEMI: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
const QUALITY: [RegExp, number[]][] = [
  [/^maj9/, [0, 4, 7, 11, 14]], [/^maj7/, [0, 4, 7, 11]], [/^m7b5/, [0, 3, 6, 10]], [/^m7/, [0, 3, 7, 10]], [/^m9/, [0, 3, 7, 10, 14]], [/^min/, [0, 3, 7]], [/^m/, [0, 3, 7]],
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

interface Line { voice: string; level: number; pan: number; steps: string[] }
interface Section { lines: Line[]; length: number }

export function parseScore(score: string): Record<string, Section> {
  const sections: Record<string, Section> = {};
  let cur = "A";
  for (const raw of score.split("\n")) {
    const head = raw.trim().match(/^\[([^\]]+)\]$/);
    if (head) {
      cur = head[1].trim();
      continue;
    }
    const i = raw.indexOf(":");
    if (i < 0) continue;
    const steps = raw.slice(i + 1).replace(/\|/g, " ").trim().split(/\s+/).filter(Boolean);
    if (!steps.length) continue;
    const m = raw.slice(0, i).trim().toLowerCase().match(/^([a-z]+)\s*(?:\(\s*([\d.]+)\s*(?:,\s*(-?[\d.]+)\s*)?\))?$/);
    if (!m) continue;
    const s = (sections[cur] ??= { lines: [], length: 0 });
    s.lines.push({ voice: m[1], level: m[2] === undefined ? 1 : Math.min(1.5, parseFloat(m[2])), pan: m[3] === undefined ? 0 : Math.max(-1, Math.min(1, parseFloat(m[3]))), steps });
    s.length = Math.max(s.length, steps.length);
  }
  return sections;
}

interface Voice { type: OscillatorType; attack: number; release: number; gain: number; shift?: number; detune?: number; overtone?: number; filter?: number }

const VOICES: Record<string, Voice> = {
  pad: { type: "sawtooth", attack: 0.5, release: 0.9, gain: 0.04, detune: 7, filter: 1400 },
  strings: { type: "sawtooth", attack: 0.25, release: 0.5, gain: 0.05, detune: 5, filter: 2600 },
  cello: { type: "sawtooth", attack: 0.12, release: 0.35, gain: 0.08, shift: -12, detune: 3, filter: 1500 },
  choir: { type: "triangle", attack: 0.4, release: 0.8, gain: 0.09, detune: 9, overtone: 0.3 },
  organ: { type: "square", attack: 0.02, release: 0.12, gain: 0.05, overtone: 0.6, filter: 2400 },
  keys: { type: "triangle", attack: 0.01, release: 0.5, gain: 0.13 },
  pluck: { type: "triangle", attack: 0.003, release: 0.3, gain: 0.16 },
  harp: { type: "triangle", attack: 0.004, release: 0.9, gain: 0.14, overtone: 0.2 },
  bell: { type: "sine", attack: 0.003, release: 1.3, gain: 0.12, shift: 12, overtone: 0.4 },
  marimba: { type: "sine", attack: 0.002, release: 0.25, gain: 0.2, overtone: 0.25 },
  brass: { type: "sawtooth", attack: 0.06, release: 0.18, gain: 0.08, filter: 1800 },
  bass: { type: "triangle", attack: 0.01, release: 0.2, gain: 0.24 },
  lead: { type: "square", attack: 0.02, release: 0.2, gain: 0.05, filter: 3000 },
  flute: { type: "sine", attack: 0.07, release: 0.2, gain: 0.15 },
};

/** The same noise every time: a small fixed generator, not a random source. */
function noiseBuffer(ctx: AudioContext, seconds: number) {
  const buf = ctx.createBuffer(1, Math.max(1, Math.floor(ctx.sampleRate * seconds)), ctx.sampleRate);
  const d = buf.getChannelData(0);
  let x = 1;
  for (let i = 0; i < d.length; i++) {
    x = (x * 16807) % 2147483647;
    d[i] = x / 1073741823.5 - 1;
  }
  return buf;
}

const SOUNDS: Record<string, { type: BiquadFilterType; freq: number; q?: number; gain: number; swell?: number }> = {
  rain: { type: "highpass", freq: 1400, gain: 0.08 },
  wind: { type: "bandpass", freq: 420, q: 0.7, gain: 0.22, swell: 0.09 },
  sea: { type: "lowpass", freq: 620, gain: 0.3, swell: 0.11 },
  river: { type: "bandpass", freq: 900, q: 0.5, gain: 0.14, swell: 0.3 },
  fire: { type: "bandpass", freq: 2400, q: 2, gain: 0.09, swell: 1.7 },
  crowd: { type: "bandpass", freq: 700, q: 1.2, gain: 0.14, swell: 0.4 },
  cave: { type: "lowpass", freq: 220, gain: 0.34, swell: 0.05 },
  night: { type: "highpass", freq: 3800, gain: 0.03, swell: 3 },
};

class Player {
  ctx: AudioContext | null = null;
  master: GainNode | null = null;
  wet: GainNode | null = null;
  dry: GainNode | null = null;
  bus: GainNode | null = null;
  timer: number | null = null;
  noise: AudioBuffer | null = null;
  amb: { gain: GainNode; nodes: AudioScheduledSourceNode[] } | null = null;
  title = $state("");
  ambienceName = $state("");
  volume = 0.5;
  muted = false;

  private ensure() {
    if (!this.ctx) {
      const ctx = (this.ctx = new AudioContext());
      this.master = ctx.createGain();
      this.master.connect(ctx.destination);
      this.dry = ctx.createGain();
      this.dry.connect(this.master);
      // A hall built from a fixed decay, so the room is the same every time.
      const conv = ctx.createConvolver();
      const n = Math.floor(ctx.sampleRate * 2.6);
      const ir = ctx.createBuffer(2, n, ctx.sampleRate);
      for (let c = 0; c < 2; c++) {
        const d = ir.getChannelData(c);
        let x = 7 + c * 13;
        for (let i = 0; i < n; i++) {
          x = (x * 16807) % 2147483647;
          d[i] = (x / 1073741823.5 - 1) * Math.pow(1 - i / n, 3);
        }
      }
      conv.buffer = ir;
      this.wet = ctx.createGain();
      this.wet.gain.value = 0.25;
      this.wet.connect(conv).connect(this.master);
      this.noise = noiseBuffer(ctx, 5);
    }
    this.setVolume(this.volume, this.muted);
    return this.ctx;
  }

  setVolume(v: number, muted: boolean) {
    this.volume = v;
    this.muted = muted;
    if (this.master) this.master.gain.value = muted ? 0 : v;
  }

  private fadeOutBus() {
    if (this.bus && this.ctx) {
      const b = this.bus;
      const t = this.ctx.currentTime;
      b.gain.cancelScheduledValues(t);
      b.gain.setValueAtTime(b.gain.value, t);
      b.gain.linearRampToValueAtTime(0, t + 1.6);
      setTimeout(() => b.disconnect(), 3000);
      this.bus = null;
    }
  }

  stop() {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    this.title = "";
    this.fadeOutBus();
  }

  private tone(time: number, dur: number, midi: number, l: Line) {
    const ctx = this.ctx!;
    const v = VOICES[l.voice] ?? VOICES.keys;
    const g = ctx.createGain();
    const peak = v.gain * l.level;
    g.gain.setValueAtTime(0, time);
    g.gain.linearRampToValueAtTime(peak, time + v.attack);
    g.gain.setValueAtTime(peak, time + Math.max(v.attack, dur));
    g.gain.exponentialRampToValueAtTime(0.0008, time + dur + v.release);
    let out: AudioNode = g;
    if (v.filter) {
      const f = ctx.createBiquadFilter();
      f.type = "lowpass";
      f.frequency.value = v.filter;
      g.connect(f);
      out = f;
    }
    const p = ctx.createStereoPanner();
    p.pan.value = l.pan;
    out.connect(p).connect(this.bus!);
    const end = time + dur + v.release + 0.03;
    const osc = (freq: number, cents: number, level: number, type: OscillatorType) => {
      const o = ctx.createOscillator();
      o.type = type;
      o.frequency.value = freq;
      o.detune.value = cents;
      const lv = ctx.createGain();
      lv.gain.value = level;
      o.connect(lv).connect(g);
      o.start(time);
      o.stop(end);
    };
    const f0 = hz(midi + (v.shift ?? 0));
    osc(f0, 0, 1, v.type);
    if (v.detune) osc(f0, v.detune, 0.7, v.type);
    if (v.overtone) osc(f0 * 2, 0, v.overtone, "sine");
  }

  private burst(time: number, dur: number, gain: number, freq: number) {
    const ctx = this.ctx!;
    const s = ctx.createBufferSource();
    s.buffer = this.noise;
    const f = ctx.createBiquadFilter();
    f.type = "highpass";
    f.frequency.value = freq;
    const g = ctx.createGain();
    g.gain.setValueAtTime(gain, time);
    g.gain.exponentialRampToValueAtTime(0.001, time + dur);
    s.connect(f).connect(g).connect(this.bus!);
    s.start(time, (time * 7) % 4, dur + 0.02);
  }

  private drum(time: number, tok: string, level: number) {
    const ctx = this.ctx!;
    for (const c of tok) {
      if (c === "k" || c === "t") {
        const o = ctx.createOscillator();
        const g = ctx.createGain();
        const f0 = c === "k" ? 120 : 200;
        o.frequency.setValueAtTime(f0, time);
        o.frequency.exponentialRampToValueAtTime(f0 / 3, time + 0.12);
        g.gain.setValueAtTime(0.4 * level, time);
        g.gain.exponentialRampToValueAtTime(0.001, time + 0.18);
        o.connect(g).connect(this.bus!);
        o.start(time);
        o.stop(time + 0.2);
      } else if (c === "s") this.burst(time, 0.14, 0.2 * level, 1500);
      else if (c === "h") this.burst(time, 0.04, 0.08 * level, 7000);
      else if (c === "o") this.burst(time, 0.22, 0.08 * level, 6000);
      else if (c === "c") this.burst(time, 0.8, 0.14 * level, 4000);
    }
  }

  play(track: { title?: string; tempo?: string | number; score?: string; form?: string; swing?: string; reverb?: string; loop?: string }) {
    const sections = parseScore(track.score ?? "");
    const names = Object.keys(sections);
    if (!names.length) return;
    const form = (track.form ?? "").split(/\s+/).filter((n) => sections[n]);
    const order = form.length ? form : names;
    this.stop();
    const ctx = this.ensure();
    void ctx.resume();
    const once = track.loop === "no" || track.loop === "false";
    const stepDur = 60 / Math.max(30, Number(track.tempo) || 90) / 2;
    const swing = Math.min(0.5, Math.max(0, Number(track.swing) || 0));
    this.wet!.gain.setTargetAtTime(Math.min(1, Math.max(0, track.reverb === undefined ? 0.25 : Number(track.reverb) || 0)) * 0.7, ctx.currentTime, 0.2);
    const bus = (this.bus = ctx.createGain());
    bus.gain.setValueAtTime(0, ctx.currentTime);
    bus.gain.linearRampToValueAtTime(1, ctx.currentTime + 1.2);
    bus.connect(this.dry!);
    bus.connect(this.wet!);
    let part = 0;
    let step = 0;
    let n = 0;
    let next = ctx.currentTime + 0.1;
    const tick = () => {
      while (next < ctx.currentTime + 0.4) {
        const sec = sections[order[part]];
        const at = next + (n % 2 ? swing * stepDur : 0);
        for (const l of sec.lines) {
          const tok = l.steps[step % l.steps.length];
          if (tok === "-" || tok === ".") continue;
          if (l.voice === "drums") {
            this.drum(at, tok, l.level);
            continue;
          }
          let hold = 1;
          while (l.steps[(step + hold) % l.steps.length] === "-" && hold < l.steps.length) hold++;
          for (const m of parseToken(tok)) this.tone(at, hold * stepDur, m, l);
        }
        n++;
        next += stepDur;
        step++;
        if (step >= sec.length) {
          step = 0;
          part++;
          if (part >= order.length) {
            if (once) {
              if (this.timer !== null) clearInterval(this.timer);
              this.timer = null;
              setTimeout(() => (this.title = ""), 1500);
              return;
            }
            part = 0;
          }
        }
      }
    };
    tick();
    this.title = track.title ?? "Untitled";
    this.timer = setInterval(tick, 120) as unknown as number;
  }

  stopAmbience() {
    this.ambienceName = "";
    if (this.amb && this.ctx) {
      const { gain, nodes } = this.amb;
      const t = this.ctx.currentTime;
      gain.gain.cancelScheduledValues(t);
      gain.gain.setValueAtTime(gain.gain.value, t);
      gain.gain.linearRampToValueAtTime(0, t + 2);
      setTimeout(() => {
        nodes.forEach((x) => x.stop());
        gain.disconnect();
      }, 2200);
      this.amb = null;
    }
  }

  ambience(a: { sounds?: string; level?: string }) {
    const names = (a.sounds ?? "").split(/[,\s]+/).filter((n) => SOUNDS[n]);
    this.stopAmbience();
    if (!names.length) return;
    const ctx = this.ensure();
    void ctx.resume();
    const level = Math.min(1, Math.max(0, a.level === undefined ? 0.5 : Number(a.level) || 0));
    const gain = ctx.createGain();
    gain.gain.setValueAtTime(0, ctx.currentTime);
    gain.gain.linearRampToValueAtTime(level, ctx.currentTime + 2.5);
    gain.connect(this.master!);
    const nodes: AudioScheduledSourceNode[] = [];
    names.forEach((name, i) => {
      const s = SOUNDS[name];
      const src = ctx.createBufferSource();
      src.buffer = this.noise;
      src.loop = true;
      const f = ctx.createBiquadFilter();
      f.type = s.type;
      f.frequency.value = s.freq;
      if (s.q) f.Q.value = s.q;
      const g = ctx.createGain();
      g.gain.value = s.gain;
      if (s.swell) {
        // Waves, gusts: a slow steady rise and fall.
        const lfo = ctx.createOscillator();
        lfo.frequency.value = s.swell;
        const depth = ctx.createGain();
        depth.gain.value = s.gain * 0.5;
        lfo.connect(depth).connect(g.gain);
        lfo.start();
        nodes.push(lfo);
      }
      src.connect(f).connect(g).connect(gain);
      src.start(0, i * 1.3);
      nodes.push(src);
    });
    this.amb = { gain, nodes };
    this.ambienceName = names.join(", ");
  }

  /** Act on what a reply asked for. */
  apply(e: { music?: any; ambience?: any }) {
    if (e.music?.stop) this.stop();
    else if (e.music?.score) this.play(e.music);
    if (e.ambience) {
      if (e.ambience.stop !== undefined || !e.ambience.sounds) this.stopAmbience();
      else this.ambience(e.ambience);
    }
  }
}

export const music = new Player();
