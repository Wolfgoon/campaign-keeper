<script lang="ts">
  import { T, tile, token, variantAt, setArt, KIND_LOOK } from "../lib/tiles";

  // The picture map: where you are and what's going on, without reading. `data` comes from the
  // host (the `tilemap` command, or the table snapshot for players).
  let { data = null, redraw }: { data: any; redraw?: () => void } = $props();
  const artCount = $derived(Object.keys(data?.art ?? {}).length);

  // The model-drawn art: each piece shows as soon as it has loaded.
  $effect(() => setArt(data?.art, () => draw()));

  let cv = $state<HTMLCanvasElement>();
  let box = $state<HTMLDivElement>();
  let zoom = $state(1);
  // Camera: the map tile at the centre of the view.
  let cam = $state({ x: 32, y: 32 });
  let placed = "";

  const tokens = $derived(Object.entries(data?.tokens ?? {}) as [string, any][]);
  const heroes = $derived(tokens.filter(([, t]) => t.kind === "hero"));
  // What the GM said that the map can't show, newest first. Ones that arrived while you watched glow.
  const notes = $derived([...(data?.notes ?? [])].reverse() as { text: string; at: number }[]);
  const opened = Math.floor(Date.now() / 1000) - 5;
  const ago = (at: number) => {
    const s = Math.max(0, Math.floor(Date.now() / 1000) - at);
    return s < 60 ? "just now" : s < 3600 ? `${Math.floor(s / 60)}m ago` : s < 86400 ? `${Math.floor(s / 3600)}h ago` : `${Math.floor(s / 86400)}d ago`;
  };
  const labels = $derived(Object.entries(data?.labels ?? {}) as [string, [number, number]][]);

  function findMe() {
    const h = heroes.length ? heroes : tokens;
    if (!h.length) return void (cam = { x: 32, y: 32 });
    cam = { x: h.reduce((s, [, t]) => s + t.x, 0) / h.length + 0.5, y: h.reduce((s, [, t]) => s + t.y, 0) / h.length + 0.5 };
  }

  // A new area (or the first one) centres on the party; within an area the view stays where you put it.
  $effect(() => {
    if (data && data.name !== placed) {
      placed = data.name;
      findMe();
    }
  });

  function draw() {
    if (!cv || !box || !data) return;
    const dpr = window.devicePixelRatio || 1;
    const W = box.clientWidth, H = box.clientHeight;
    if (cv.width !== Math.round(W * dpr) || cv.height !== Math.round(H * dpr)) {
      cv.width = Math.round(W * dpr);
      cv.height = Math.round(H * dpr);
    }
    const c = cv.getContext("2d")!;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.imageSmoothingEnabled = false;
    c.fillStyle = "#07070b";
    c.fillRect(0, 0, W, H);
    const s = T * zoom;
    const ox = W / 2 - cam.x * s, oy = H / 2 - cam.y * s;
    const x0 = Math.max(0, Math.floor(-ox / s)), y0 = Math.max(0, Math.floor(-oy / s));
    const x1 = Math.min(data.size, Math.ceil((W - ox) / s)), y1 = Math.min(data.size, Math.ceil((H - oy) / s));
    for (let y = y0; y < y1; y++) {
      const row: string = data.rows[y] ?? "";
      for (let x = x0; x < x1; x++) {
        const name = data.terrain[row[x] ?? " "] ?? "void";
        c.drawImage(tile(name, variantAt(x, y)), Math.floor(ox + x * s), Math.floor(oy + y * s), Math.ceil(s), Math.ceil(s));
      }
    }
    const text = (t: string, x: number, y: number, size: number, col: string) => {
      c.font = `bold ${size}px system-ui, sans-serif`;
      c.textAlign = "center";
      c.lineWidth = Math.max(2, size / 4);
      c.strokeStyle = "rgba(0,0,0,.85)";
      c.strokeText(t, x, y);
      c.fillStyle = col;
      c.fillText(t, x, y);
    };
    for (const [t, [x, y]] of labels) text(t, ox + (x + 0.5) * s, oy + (y + 0.5) * s, Math.max(11, 18 * zoom), "#fff7d6");
    for (const [name, t] of tokens) {
      const X = ox + t.x * s, Y = oy + t.y * s;
      c.drawImage(token(t.kind, name), Math.floor(X), Math.floor(Y), Math.ceil(s), Math.ceil(s));
      text(name, X + s / 2, Y + s + Math.max(10, 13 * zoom), Math.max(10, 13 * zoom), KIND_LOOK[t.kind]?.col ?? "#fff");
    }
  }

  $effect(() => {
    // Redraw whenever anything it reads changes.
    void [data, zoom, cam.x, cam.y];
    draw();
  });

  // The view box only exists once there's a map, which can arrive after this mounts.
  $effect(() => {
    if (!box) return;
    const ro = new ResizeObserver(() => draw());
    ro.observe(box);
    return () => ro.disconnect();
  });

  // Drag to look around; wheel or buttons to zoom.
  let drag: { x: number; y: number; cx: number; cy: number } | null = null;
  function down(e: PointerEvent) {
    drag = { x: e.clientX, y: e.clientY, cx: cam.x, cy: cam.y };
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!drag || !data) return;
    const s = T * zoom;
    cam = {
      x: Math.min(data.size, Math.max(0, drag.cx - (e.clientX - drag.x) / s)),
      y: Math.min(data.size, Math.max(0, drag.cy - (e.clientY - drag.y) / s)),
    };
  }
  const ZOOMS = [0.25, 0.5, 0.75, 1, 1.5, 2];
  function zoomBy(d: number) {
    const i = ZOOMS.indexOf(zoom);
    zoom = ZOOMS[Math.min(ZOOMS.length - 1, Math.max(0, (i < 0 ? 3 : i) + d))];
  }
  function wheel(e: WheelEvent) {
    e.preventDefault();
    zoomBy(e.deltaY < 0 ? 1 : -1);
  }
  function lookAt(t: any) {
    cam = { x: t.x + 0.5, y: t.y + 0.5 };
  }
</script>

{#if !data}
  <div class="card col">
    <h2 style="margin:0">🗺 No picture map yet</h2>
    <p class="muted">It draws itself as you play: after each reply from the GM, a quick helper model paints where you are and who's around.</p>
  </div>
{:else}
  <div class="tm col">
    <div class="tm-banner">
      <div class="tm-place">📍 {data.name || "Somewhere…"}</div>
      {#if data.now}<div class="tm-now">{data.now}</div>{/if}
      {#if artCount < 28}<div class="small" style="opacity:.75">🎨 The art model is drawing this world's tiles ({artCount} of 28)…</div>{/if}
    </div>
    <div class="tm-view" bind:this={box}>
      <canvas bind:this={cv} onpointerdown={down} onpointermove={move} onpointerup={() => (drag = null)} onpointercancel={() => (drag = null)} onwheel={wheel} aria-label="Picture map of {data.name}"></canvas>
      <div class="tm-tools">
        <button onclick={() => zoomBy(1)} aria-label="Zoom in">＋</button>
        <button onclick={() => zoomBy(-1)} aria-label="Zoom out">－</button>
        <button onclick={findMe} aria-label="Find me">⌖</button>
        {#if redraw}<button onclick={redraw} aria-label="Redraw the map art" title="Throw the art away and have the model draw it all again">🎨</button>{/if}
      </div>
    </div>
    {#if notes.length}
      <div class="tm-notes" aria-label="What the GM said">
        <div class="small muted">🔔 Things to know</div>
        {#each notes as n}
          <div class="tm-note" class:fresh={n.at >= opened}><span class="grow">{n.text}</span><span class="small muted">{ago(n.at)}</span></div>
        {/each}
      </div>
    {/if}
    {#if tokens.length}
      <div class="tm-who">
        {#each tokens as [name, t]}
          <button class="ghost small" style:border-color={KIND_LOOK[t.kind]?.col} onclick={() => lookAt(t)} title={KIND_LOOK[t.kind]?.word}>
            {KIND_LOOK[t.kind]?.icon ?? "•"} {name}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .tm { flex: 1; min-height: 0; gap: 0.5rem; }
  .tm-banner { padding: 0.6rem 0.9rem; border-radius: 10px; background: linear-gradient(90deg, #1e293b, #334155); color: #fff; }
  .tm-place { font-size: 1.35rem; font-weight: 800; }
  .tm-now { font-size: 1.05rem; opacity: 0.92; margin-top: 0.15rem; }
  .tm-view { position: relative; flex: 1; min-height: 320px; border-radius: 10px; overflow: hidden; background: #07070b; }
  .tm-view canvas { position: absolute; inset: 0; width: 100%; height: 100%; touch-action: none; cursor: grab; image-rendering: pixelated; }
  .tm-tools { position: absolute; right: 0.5rem; top: 0.5rem; display: flex; flex-direction: column; gap: 0.3rem; }
  .tm-tools button { width: 2.6rem; height: 2.6rem; font-size: 1.3rem; padding: 0; opacity: 0.9; }
  .tm-notes { display: flex; flex-direction: column; gap: 0.3rem; max-height: 30vh; overflow-y: auto; }
  .tm-note { display: flex; gap: 0.5rem; align-items: baseline; padding: 0.45rem 0.7rem; border-radius: 8px; background: var(--card, rgba(127, 127, 127, 0.12)); border-left: 4px solid #64748b; }
  .tm-note.fresh { border-left-color: #facc15; background: rgba(250, 204, 21, 0.14); }
  .tm-who { display: flex; flex-wrap: wrap; gap: 0.35rem; }
  .tm-who button { border: 2px solid; }
</style>
