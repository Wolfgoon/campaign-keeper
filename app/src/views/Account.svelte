<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { call, toast } from "../lib/api";
  import { writeText } from "../lib/clipboard";

  // The player's Google account, and the stories kept in it. `open` opens a campaign folder;
  // Settings shows the card without the list of stories.
  let { open = (_path: string) => {}, list = true } = $props();
  let acct = $state<any>(null);
  let signing = $state<any>(null);
  let stories = $state<any[] | null>(null);
  let fetching = $state("");
  let timer: any;

  async function load() {
    acct = await call("account_status");
    if (acct.email && list) refresh();
  }

  async function refresh() {
    try {
      stories = await call("account_stories");
    } catch (e) {
      toast(e);
      stories = stories ?? [];
    }
  }

  async function signIn() {
    try {
      signing = await call("account_begin");
      wait();
    } catch (e) {
      toast(e);
    }
  }

  // Google says how often to ask whether the player has finished typing the code.
  function wait() {
    timer = setTimeout(async () => {
      if (!signing) return;
      try {
        const email = await call("account_poll", { deviceCode: signing.device_code });
        if (email) {
          signing = null;
          toast(`Signed in as ${email}.`);
          await load();
        } else wait();
      } catch (e) {
        signing = null;
        toast(e);
      }
    }, (signing.interval + 1) * 1000);
  }

  function cancel() {
    clearTimeout(timer);
    signing = null;
  }

  async function signOut() {
    if (!confirm("Sign out? Your stories stay on this device and in your account.")) return;
    await call("account_sign_out");
    stories = null;
    await load();
  }

  async function get(s: any) {
    if (s.path) return open(s.path);
    fetching = s.story.file_id;
    try {
      open(await call<string>("account_fetch", { fileId: s.story.file_id }));
    } catch (e) {
      toast(e);
    }
    fetching = "";
  }

  async function remove(s: any) {
    if (!confirm(`Remove "${s.story.name}" from your account? Copies on your devices stay where they are.`)) return;
    try {
      await call("account_remove", { fileId: s.story.file_id });
    } catch (e) {
      toast(e);
    }
    refresh();
  }

  function when(iso: string) {
    const d = new Date(iso);
    return isNaN(+d) ? "" : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
  }

  onMount(() => {
    load();
    return () => clearTimeout(timer);
  });
</script>

{#if acct?.configured}
  <div class="card col">
    {#if signing}
      <h3 style="margin:0">Sign in with Google</h3>
      <p class="small" style="margin:0">1. Open <b>{signing.verification_url.replace("https://", "")}</b> on this or any other device.<br />2. Enter this code:</p>
      <div class="mono" style="font-size:24px;letter-spacing:.08em">{signing.user_code}</div>
      <div class="row">
        <button class="primary" onclick={() => { writeText(signing.user_code).catch(() => {}); openUrl(signing.verification_url).catch(toast); }}>Copy the code and open the page</button>
        <button onclick={cancel}>Cancel</button>
      </div>
      <p class="small muted" style="margin:0">Waiting for you to finish there… Campaign Keeper can only see the files it saves itself, in a "Campaign Keeper" folder in your Google Drive.</p>
    {:else if !acct.email}
      <h3 style="margin:0">☁ Your stories on every device</h3>
      <p class="small muted" style="margin:0">Sign in with Google and each story is kept in your own Google Drive, ready to pick up on your phone or another computer.</p>
      <button class="primary" onclick={signIn}>Sign in with Google…</button>
    {:else}
      <div class="row">
        <h3 class="grow" style="margin:0">☁ Your account</h3>
        {#if list}<button class="ghost small" onclick={refresh}>Refresh</button>{/if}
        <button class="ghost small" onclick={signOut}>Sign out</button>
      </div>
      <p class="small muted" style="margin:0">{acct.email}. Stories you open are kept in step with your account.</p>
      {#if !list}
        <!-- Settings: just who's signed in. -->
      {:else if stories === null}<p class="small muted" style="margin:0">Looking in your account…</p>
      {:else if !stories.length}<p class="small muted" style="margin:0">No stories in your account yet. Open or start one and it's added.</p>{/if}
      {#each stories ?? [] as s (s.story.file_id)}
        <div class="recent">
          <button disabled={!!fetching} onclick={() => get(s)}>
            {s.story.name}
            <span class="muted small"> · {fetching === s.story.file_id ? "downloading…" : s.path ? "on this device" : "download"} · {when(s.story.modified)}</span>
          </button>
          <button class="ghost" aria-label="Remove {s.story.name} from your account" onclick={() => remove(s)}>✖</button>
        </div>
      {/each}
    {/if}
  </div>
{:else if acct && !list}
  <p class="small error">Google sign-in isn't set up in this copy yet. Add a Google client below to turn it on.</p>
{/if}
