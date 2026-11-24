// The picture map's art. Every tile and token is a 64x64 sprite a model drew for this campaign and
// the app reduced with k-means (see tileart.rs); this file only shows it. Until a piece arrives the
// map uses a plain colour so you can still read it.

export const T = 64;

/** A tile's variant from where it sits, so neighbours differ but nothing flickers. */
export function variantAt(x: number, y: number) {
  return (Math.imul(x, 73856093) ^ Math.imul(y, 19349663)) >>> 0;
}

// Natural ground can be mirrored without looking wrong, which breaks up the repeat.
const MIRROR = new Set(["grass", "forest", "water", "deep", "sand", "dirt", "snow", "swamp", "lava", "rock", "farm", "road"]);

const STAND_IN: Record<string, string> = {
  void: "#07070b", grass: "#4f9a3c", forest: "#2a6a2a", water: "#2f6fc0", deep: "#173f7a", sand: "#e3cc8a", road: "#8a8070",
  dirt: "#8a6238", floor: "#9c6b3d", wall: "#5a5a66", mountain: "#7d7a78", snow: "#e9f0f8", swamp: "#4c5c2c", lava: "#c8401a",
  bridge: "#8a5a2e", door: "#6b3f1e", building: "#a33a2a", cave: "#2a262c", farm: "#7a5230", rock: "#6c6862",
};

export const KIND_LOOK: Record<string, { col: string; icon: string; word: string }> = {
  hero: { col: "#3b82f6", icon: "🧝", word: "You / your party" },
  ally: { col: "#22c55e", icon: "🤝", word: "Friend" },
  npc: { col: "#eab308", icon: "🙂", word: "Person" },
  enemy: { col: "#ef4444", icon: "⚔", word: "Enemy" },
  monster: { col: "#a21caf", icon: "👹", word: "Monster" },
  item: { col: "#f59e0b", icon: "✨", word: "Item" },
  chest: { col: "#b45309", icon: "📦", word: "Chest" },
  camp: { col: "#f97316", icon: "⛺", word: "Camp" },
  quest: { col: "#facc15", icon: "❗", word: "Go here" },
};

function canvas(draw: (c: CanvasRenderingContext2D) => void) {
  const cv = document.createElement("canvas");
  cv.width = cv.height = T;
  const c = cv.getContext("2d")!;
  c.imageSmoothingEnabled = false;
  draw(c);
  return cv;
}

const cache = new Map<string, HTMLCanvasElement>();
let urls: Record<string, string> = {};
let onReady: () => void = () => {};

/** Hand over the art the host has (name -> PNG data URL); `ready` redraws once pieces have loaded. */
export function setArt(art: Record<string, string> | undefined, ready: () => void) {
  onReady = ready;
  for (const [name, url] of Object.entries(art ?? {})) {
    if (urls[name] === url || !url.startsWith("data:image/png;base64,")) continue;
    urls[name] = url;
    const img = new Image();
    img.onload = () => {
      for (const flip of [false, true]) {
        cache.set(`${name}:${flip}`, canvas((c) => {
          if (flip) { c.translate(T, 0); c.scale(-1, 1); }
          c.drawImage(img, 0, 0, T, T);
        }));
      }
      onReady();
    };
    img.src = url;
  }
}

function standIn(key: string, draw: (c: CanvasRenderingContext2D) => void) {
  let cv = cache.get(key);
  if (!cv) cache.set(key, (cv = canvas(draw)));
  return cv;
}

export function tile(name: string, at: number): HTMLCanvasElement {
  const art = cache.get(`${name}:${MIRROR.has(name) && at % 2 === 1}`);
  if (art) return art;
  return standIn(`plain:${name}`, (c) => { c.fillStyle = STAND_IN[name] ?? "#07070b"; c.fillRect(0, 0, T, T); });
}

/** Same as tileart.rs `slug`, so a named sprite is found by its name. */
export function slug(name: string) {
  return [...name.trim().toLowerCase()].map((c) => (/[\p{L}\p{N}]/u.test(c) ? c : "_")).slice(0, 60).join("");
}

/** A named character's or thing's own sprite when it has one, else the kind's. */
export function token(kind: string, name = ""): HTMLCanvasElement {
  const art = (name && cache.get(`token:${slug(name)}:false`)) || cache.get(`${kind}:false`);
  if (art) return art;
  return standIn(`plain-token:${kind}`, (c) => {
    c.fillStyle = "#111";
    c.beginPath(); c.arc(32, 32, 20, 0, Math.PI * 2); c.fill();
    c.fillStyle = KIND_LOOK[kind]?.col ?? "#fff";
    c.beginPath(); c.arc(32, 32, 17, 0, Math.PI * 2); c.fill();
  });
}
