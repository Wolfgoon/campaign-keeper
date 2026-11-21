// The player's screen in a plain web browser, served by the host's app. There's no Campaign Keeper
// behind the page here, so the few things the player's screen asks for are answered in the browser:
// settings live in this browser's storage, and the table is reached over a WebSocket to the host.

// Must match `PROTOCOL` in src-tauri/src/net/mod.rs: the host refuses a page that speaks another
// version, and a browser can't "update the app". `the_web_page_speaks_the_hosts_protocol` checks it.
const PROTOCOL = 3;
const CFG = "ck-web-cfg";
const TOKEN = "ck-web-token";
/** The host sends a heartbeat every 15 seconds, so this much silence means the line is dead. */
const QUIET = 45_000;

const defaults = { player_name: "", join_addr: "", music_volume: 0.5, music_muted: false, tts_auto: false, tts_speed: 1, dark_mode: true, ui_scale: 1 };

function stored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private browsing: it just lasts this visit */
  }
}

/** Who this browser is to a table, so a dropped player gets their seat back. */
function token(): string {
  let t = stored(TOKEN);
  if (!t) {
    const b = new Uint8Array(24);
    crypto.getRandomValues(b);
    t = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
    store(TOKEN, t);
  }
  return t;
}

const listeners = new Set<(m: any) => void>();
const emit = (m: any) => listeners.forEach((f) => f(m));

export function onClient(f: (m: any) => void): () => void {
  listeners.add(f);
  return () => listeners.delete(f);
}

let ws: WebSocket | null = null;

function leave() {
  const old = ws;
  ws = null;
  old?.close();
}

function join(code: string, name: string): Promise<void> {
  if (!name.trim()) return Promise.reject("Enter your name to join.");
  if (code.replace(/[^a-z0-9]/gi, "").length !== 8) return Promise.reject("The join code has 8 letters and numbers, like K7Q2-M9XD.");
  leave();
  const sock = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`);
  ws = sock;
  return new Promise((resolve, reject) => {
    let answered = false;
    let over = false;
    let last = Date.now();
    const beat = setInterval(() => {
      if (sock.readyState !== WebSocket.OPEN) return;
      if (Date.now() - last > QUIET) sock.close();
      else sock.send('{"t":"ping"}');
    }, 15_000);
    sock.onopen = () => sock.send(JSON.stringify({ t: "hello", name: name.trim(), token: token(), protocol: PROTOCOL, code, app: "web", pings: true }));
    sock.onmessage = (e) => {
      last = Date.now();
      let m: any;
      try {
        m = JSON.parse(e.data);
      } catch {
        return;
      }
      if (m.t === "ping") return;
      if (m.t === "goodbye" || m.t === "refused") over = true;
      if (!answered) {
        answered = true;
        // Turned away at the door: that's the answer to Join, not news from a table.
        if (m.t === "refused") return reject(m.reason);
        resolve();
      }
      emit(m);
    };
    sock.onclose = () => {
      clearInterval(beat);
      // Left on purpose, or replaced by a newer connection: nothing to report.
      if (ws !== sock) return;
      ws = null;
      if (!answered) {
        answered = true;
        reject("The host didn't answer. The table may be closed, or this device may be blocked after too many wrong join codes.");
      } else if (!over) emit({ t: "goodbye", reason: "The connection to the host was lost.", lost: true });
    };
  });
}

export async function webCall(cmd: string, args: any = {}): Promise<any> {
  switch (cmd) {
    case "get_config":
      try {
        return { ...defaults, ...JSON.parse(stored(CFG) ?? "{}") };
      } catch {
        return { ...defaults };
      }
    case "set_config":
      return store(CFG, JSON.stringify(args.cfg));
    case "app_info":
      return { mobile: matchMedia("(pointer: coarse)").matches, library: "", web: true };
    case "table_scan":
      return [];
    case "client_join":
      return join(args.code, args.name);
    case "client_send":
      if (ws?.readyState !== WebSocket.OPEN) throw "The host closed the connection.";
      return ws.send(JSON.stringify(args.msg));
    case "client_leave":
      return leave();
    default:
      throw `"${cmd}" isn't available in a web browser.`;
  }
}
