<script lang="ts">
  import { call, toast } from "../lib/api";

  let { table = $bindable(null), cfg } = $props();
  let gm = $state(false);
  let announce = $state(true);
  let talk = $state("");
  let newChar = $state("");

  async function open() {
    try {
      await call("table_open", { hostIsGm: gm, announce });
      table = await call("table_state");
    } catch (e) {
      toast(e);
    }
  }

  async function close() {
    if (!confirm("Close the table? This disconnects everyone. The game stays saved here.")) return;
    await call("table_close");
    table = null;
  }

  async function claim(character: string, isNew: boolean) {
    try {
      await call("table_host_character", { character, new: isNew });
      newChar = "";
      table = await call("table_state");
    } catch (e) {
      toast(e);
    }
  }

  const seats = $derived(table?.table?.seats ?? []);
  const asking = $derived(seats.filter((s: any) => !s.admitted));
  const seated = $derived(seats.filter((s: any) => s.admitted));
</script>

{#if !table}
  <div class="col">
    <h3>Play together</h3>
    <p class="small muted">Friends join from their own Campaign Keeper. The AI runs here, and waits for every player's action before each reply. Over the internet, use a VPN such as Tailscale.</p>
    <label class="field"><span>Your name</span><input bind:value={cfg.player_name} onchange={() => call("set_config", { cfg: $state.snapshot(cfg) })} /></label>
    <label class="check"><input type="radio" name="gmrole" checked={!gm} onchange={() => (gm = false)} style="width:auto" /> <span>I'll play a character (the AI is the Game Master)</span></label>
    <label class="check"><input type="radio" name="gmrole" checked={gm} onchange={() => (gm = true)} style="width:auto" /> <span>I'll be the Game Master (I narrate and plan with the AI)</span></label>
    <label class="check"><input type="checkbox" bind:checked={announce} /> <span>Show this table on the local network. Announces the campaign and your name, never the code.</span></label>
    <button class="primary" onclick={open}>👥 Open the table</button>
  </div>
{:else}
  <div class="col">
    <div class="card">
      <div class="small muted">Join code</div>
      <div class="mono" style="font-size:20px;letter-spacing:.06em">{table.code}</div>
      <div class="small muted">Players join with your address (port {table.port}) and this code.</div>
    </div>
    <label class="check small"><input type="checkbox" checked={table.auto_admit} onchange={(e) => call("table_auto_admit", { on: e.currentTarget.checked })} /> <span>Let people in without asking</span></label>
    {#if !table.host_is_gm}
      <label class="field"><span>Your character</span>
        <select value={table.host_character} onchange={(e) => claim(e.currentTarget.value, false)}>
          <option value="">(none)</option>
          {#each table.characters ?? [] as c}<option value={c}>{c}</option>{/each}
        </select>
      </label>
      <div class="row">
        <input class="grow" style="width:auto" bind:value={newChar} placeholder="Or a new character's name" aria-label="New character's name" />
        <button disabled={!newChar.trim()} onclick={() => claim(newChar, true)}>Make & take</button>
      </div>
    {/if}
    {#if asking.length}
      <h3>Asking to join</h3>
      {#each asking as s}
        <div class="row small">
          <span class="grow">{s.name}</span>
          <button class="primary" onclick={() => call("table_decide", { id: s.id, admit: true, block: false })}>Let in</button>
          <button onclick={() => call("table_decide", { id: s.id, admit: false, block: false })}>Turn away</button>
        </div>
      {/each}
    {/if}
    <h3>At the table</h3>
    {#if !seated.length}<p class="small muted">Nobody yet.</p>{/if}
    {#each seated as s}
      <div class="row small">
        <span>{s.connected ? "●" : "○"}</span>
        <span class="grow">{s.name}{s.character ? ` as ${s.character}` : " · choosing…"}</span>
        <span>{s.acted ? "✔ acted" : s.character ? "deciding" : ""}</span>
        <button class="ghost danger" aria-label="Remove {s.name}" onclick={() => call("table_decide", { id: s.id, admit: false, block: false })}>✖</button>
        <button class="ghost danger small" title="Remove, and refuse their address until the table closes" onclick={() => { if (confirm(`Block ${s.name} until the table closes?`)) call("table_decide", { id: s.id, admit: false, block: true }); }}>Block</button>
      </div>
    {/each}
    <button onclick={() => call("table_send_now").catch(toast)} title="Don't wait for everyone; players who haven't acted sit this round out">▶ Send the round now</button>
    <h3>Table talk</h3>
    <p class="small muted">Out of character. The AI never sees it.</p>
    <div class="col small">
      {#each table.table?.talk ?? [] as l}<div><b>{l.from}:</b> {l.text}</div>{/each}
    </div>
    <input bind:value={talk} placeholder="Say something to the table" aria-label="Say something to the table" onkeydown={(e) => { if (e.key === "Enter") { call("table_talk", { text: talk }); talk = ""; } }} />
    <button class="ghost danger" onclick={close}>Close the table</button>
  </div>
{/if}
