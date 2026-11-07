<script lang="ts">
  import { onMount } from "svelte";
  import { call, md, toast } from "../lib/api";

  let files = $state<string[]>([]);
  let filter = $state("");
  let path = $state("");
  let text = $state("");
  let disk = $state("");
  let editing = $state(false);
  let changedOnDisk = $state(false);
  let timer: any;

  const shown = $derived(files.filter((f) => f.toLowerCase().includes(filter.toLowerCase())));
  const dirs = $derived.by(() => {
    const m = new Map<string, string[]>();
    for (const f of shown) {
      const i = f.lastIndexOf("/");
      const d = i < 0 ? "" : f.slice(0, i);
      if (!m.has(d)) m.set(d, []);
      m.get(d)!.push(f);
    }
    return [...m.entries()];
  });
  const dirty = $derived(text !== disk);

  async function load() {
    files = await call("list_files");
  }

  async function pick(f: string) {
    if (dirty && !confirm("Discard your unsaved changes?")) return;
    path = f;
    text = disk = await call("read_file", { path: f });
    changedOnDisk = false;
  }

  async function save() {
    await call("write_file", { path, text });
    disk = text;
    toast("Saved.");
  }

  async function rename() {
    const to = prompt("New path", path);
    if (!to || to === path) return;
    try {
      await call("rename_file", { from: path, to });
      path = to;
      await load();
    } catch (e) {
      toast(e);
    }
  }

  async function trash() {
    if (!confirm(`Move ${path} to the campaign's trash (.keeper/trash)? You can get it back later.`)) return;
    await call("trash_file", { path });
    path = "";
    text = disk = "";
    await load();
  }

  // Someone else (the Scribe, another editor) may change a file while it's open.
  async function watch() {
    if (!path) return;
    try {
      const now = await call<string>("read_file", { path });
      changedOnDisk = now !== disk;
    } catch {
      /* moved or deleted: the next action says so */
    }
  }

  onMount(() => {
    load();
    timer = setInterval(watch, 3000);
    return () => clearInterval(timer);
  });
</script>

<div class="split">
  <div class="tree">
    <div style="padding:8px"><input bind:value={filter} placeholder="🔍 Filter files" aria-label="Filter files" /></div>
    <div class="list">
      {#each dirs as [d, list]}
        {#if d}<div class="dir">{d}</div>{/if}
        {#each list as f}
          <button class:on={f === path} title={f} onclick={() => pick(f)}>{f.split("/").pop()}</button>
        {/each}
      {/each}
    </div>
  </div>
  <div class="editor">
    {#if path}
      <div class="row" style="padding:6px 12px;border-bottom:1px solid var(--line)">
        <b class="grow small">{path}{dirty ? " • unsaved" : ""}</b>
        <button onclick={() => (editing = !editing)}>{editing ? "👁 Read" : "✏ Edit"}</button>
        <button class="primary" disabled={!dirty} onclick={save}>Save</button>
        <button disabled={!dirty} onclick={() => (text = disk)}>Revert</button>
        <button onclick={rename}>Rename…</button>
        <button class="danger" onclick={trash}>Trash…</button>
      </div>
      {#if changedOnDisk}
        <div class="row small" style="padding:6px 12px;background:var(--panel-2)">
          <span class="grow">This file changed on disk while you were editing it.</span>
          <button onclick={() => pick(path)}>Load the disk version</button>
          <button onclick={async () => { disk = await call("read_file", { path }); changedOnDisk = false; }}>Keep mine</button>
        </div>
      {/if}
      {#if editing}
        <textarea bind:value={text} aria-label="File text"></textarea>
      {:else}
        <div class="reader">{@html md(text)}</div>
      {/if}
    {:else}
      <div class="pad muted">Pick a file on the left to read or edit it.</div>
    {/if}
  </div>
</div>
