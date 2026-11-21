# Handoff: Campaign Keeper 3.0.1 (AD System LitRPG mode + Tailscale pairing)

Repo: `C:\Users\patri\OneDrive\Documents\GitHub\campaign-keeper` (Tauri 2 + Svelte 5 + Rust). Windows, PowerShell.
Nothing is committed yet. There were already uncommitted changes before this work: don't revert anything.

## What was done
- **New ruleset "AD System"**: Spirit's leveling, magic (frequencies) and crafting, ported from
  `..\Spirit\game-engine\engine\system.py`, `frequency.py`, `semi.py` and `design\AD-magic-crafting-world.md`.
  **Never edit anything in the Spirit folder.**
  - `app/src-tauri/src/ad.rs`: the engine (exact big numbers via num-bigint/num-rational, 22 tests).
  - `app/src-tauri/ad/*.json`: copies of Spirit's data files.
  - `app/src-tauri/prompts/litrpg_ad.md`: the GM's ("the System's") instructions.
  - `rpg.rs` `Rules` has `ruleset` ("" = Classic, "ad") and `ad: ad::World`, plus helpers `is_ad, has_sheets,
    names, status_all, apply, ensure_sheet, view`. Classic mode is unchanged.
  - Wired in `game.rs` (status window, `<rpg>` lines, no referee checks in AD), `prompt.rs`, `lib.rs`
    (`rpg_enable(ruleset)`, new command `rpg_views`), `net/table.rs` (multiplayer), `views/Status.svelte`
    (ruleset choice + AD panel).
  - The GM can rename System words: `rename amplitude -> Aether` (display only; numbers never change).
  - Lines starting with `ad::SYSTEM_ONLY` ("§ ") go only to the GM (rpg_notices), never to the player.
- **Version bumped to 3.0.0** in `app/package.json`, `app/src-tauri/tauri.conf.json`, `app/src-tauri/Cargo.toml`.
- **Multiplayer**: `PROTOCOL` bumped 2 -> 3 in `app/src-tauri/src/net/mod.rs`, so a 2.x app is refused with
  "update the app" instead of breaking on the new sheet format. Browser players get the host's own built-in UI,
  so they always match. New test: `net::table_tests::the_ad_system_reaches_players_and_only_they_choose_for_themselves`.
- Docs: `How to install.txt` and `README.md` now point at 3.0.0, say everyone at a table needs 3.0+, and have a
  section on playing from a phone with the computer's Claude Code over Tailscale.

## Test status
- `cd app/src-tauri; cargo test --lib`: everything passes. `net::table_tests::a_round_waits_for_everyone_here_and_nobody_else`
  failed once from a timing race inside that old test (it passes 5/5 on its own). It wasn't caused by this work.
- `cd app; npx svelte-check --threshold error`: 0 errors.

## Done since this handoff was written
1. **Windows installer**: `installers/Campaign Keeper Setup 3.0.0.exe` (7,299,173 bytes, ProductVersion 3.0.0),
   copied from `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.0_x64-setup.exe`.
2. **Android APK**: `installers/Campaign Keeper 3.0.0 (Android).apk` (23,894,569 bytes, versionCode 3000000,
   versionName 3.0.0), built with `npx tauri android build --apk --target aarch64` (env vars set as below). The build
   failed at the "Creation symbolic link is not allowed for this system" step (no Windows Developer Mode), so the
   documented fallback was used: the freshly built
   `src-tauri/target/aarch64-linux-android/release/libcampaign_keeper_lib.so` was copied to
   `gen/android/app/src/main/jniLibs/arm64-v8a/` and `gradlew assembleArm64Release -x rustBuildArm64Release` finished
   the APK. `apksigner verify --print-certs` shows `CN=Campaign Keeper`, SHA-256
   `9b4b266b2faa4857bbcbaf9aa457f76800f3bfeb5368e11b91ca2b3b109832a2` — the same key as the 2.3.0 APK, so it
   upgrades in place.
   Note: the symlink failure had to be finished by hand because the build was driven from PowerShell and the log was
   written as UTF-16 (`Tee-Object`), so reading it back with `Get-Content -Raw` for the fallback check garbled the
   text. `app/build-android.sh` greps the log for "symbolic link", which this message does contain, so it should still
   take the fallback when run from Git Bash — that path was not re-run this time.
3. Test status re-checked: `cargo test --lib` -> 132 passed, 0 failed, 2 ignored (includes the 22 `ad::tests` and
   `net::table_tests::the_ad_system_reaches_players_and_only_they_choose_for_themselves`).
   `npx svelte-check --threshold error` -> 0 errors, 4 warnings.

## Pairing, and fixes found while testing multiplayer (after the 3.0.0 artifacts were staged)

- **Tailscale pairing, one paste.** `app/src-tauri/src/net/pair.rs` (new):
  `campaignkeeper://join?host=my-pc&port=47120&code=K7Q2-M9XD` carries the address, the port and the
  join code. `pair::parse` also understands the older human invitation ("open http://my-pc:47122 and
  enter the code …"), normalizes the code, and rejects junk with a message a player can act on.
  - Host side: `Table::state()` now includes `pairing` (`text`, `host`, `over`, and `others`). Routes
    are ordered Tailscale name first (detected by aiming a UDP socket at Tailscale's own DNS,
    `100.100.100.100`, and checking the local address is in 100.64.0.0/10), then the Tailscale
    address, then the LAN address. `net::ip_towards` is the shared helper; `local_ip` uses it too.
  - Player side: new command `pair_parse`; `client_join` also accepts a whole invitation pasted into
    the address field. The join screen has an "Invitation (paste what the host copied)" field that
    fills in both fields, and the Table panel has a Pair card with "Copy the invitation" / "Copy a
    message".
  - Docs: `README.md` and `How to install.txt` now tell phone players to paste the invitation.
- **Bug fixed: browser players were refused.** `app/src/lib/web.ts` still sent `protocol: 2` after
  `PROTOCOL` was bumped to 3, so every browser tab was turned away with "update the app" — which a
  browser cannot do. It is 3 now, and `net::table_tests::the_web_page_speaks_the_hosts_protocol`
  reads `web.ts` and fails if the two ever drift apart again.
- **Two timing races in `net/table_tests.rs` fixed** (both pre-existing, both flaky under load):
  `players_cannot_narrate_command_or_act_as_nobody` now waits for the character sheet file instead of
  racing its write; `a_round_waits_for_everyone_here_and_nobody_else` waits for Ann's replacement
  action before Bob's pass lets the round go.
- **DeepSeek V4 Flash**: the preset in Settings is now `https://api.deepseek.com/v1` +
  `deepseek-v4-flash`. `engine/openai.rs` also accepts `DEEPSEEK_API_KEY` from the environment when
  the Settings key is empty (test: `engine::openai::tests::a_deepseek_key_can_come_from_the_environment`).
  This machine's `%APPDATA%\campaign-keeper\config.json` was switched to `engine: "other"` with that
  address and model (backup: `config.json.before-deepseek`). **No key was available to store**, so
  paste it in Settings → Other models, or set `DEEPSEEK_API_KEY`, before starting a game.

Test status after this work:
- `cd app/src-tauri; cargo test --lib` -> **143 passed, 0 failed, 2 ignored** (7 new `net::pair` tests,
  3 new table tests, 1 new engine test).
- `cd app; npx svelte-check --threshold error` -> 0 errors, 4 warnings.
- `cd app; npm run build` -> rebuilt `dist/` (needed: `net/web.rs` embeds it, and the protocol fix
  lives there; the built bundle now sends `protocol: 3`).

**The `installers/*3.0.0*` files predate this work**, so the version was bumped and both installers
were rebuilt:

## Version 3.0.1 and the rebuilt artifacts

Bumped to **3.0.1** in `app/package.json`, `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock`
and `app/src-tauri/tauri.conf.json`. The Android CLI regenerated
`app/src-tauri/gen/android/app/tauri.properties` as `versionCode=3000001`, `versionName=3.0.1`.

1. **Windows installer**: `installers/Campaign Keeper Setup 3.0.1.exe` — 7,282,485 bytes,
   ProductVersion and FileVersion 3.0.1, SHA-256
   `e47fcb89739243c0bc523791f3529045259eeefdd7ca4ce3cff77b83395b2d48`. Built with
   `cd app; npm run tauri build` (NSIS, currentUser), copied from
   `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.1_x64-setup.exe`.
2. **Android APK**: `installers/Campaign Keeper 3.0.1 (Android).apk` — 23,857,389 bytes, versionCode
   3000001, versionName 3.0.1, SHA-256
   `8c96c1815bfe9607f068ab09484b2e38573172001be8982d037d4b962e8216b3`.
   `npx tauri android build --apk --target aarch64` stopped again at "Creation symbolic link is not
   allowed for this system" (no Developer Mode). Fallback: the fresh
   `src-tauri/target/aarch64-linux-android/release/libcampaign_keeper_lib.so` (22,151,328 bytes) was
   copied to `gen/android/app/src/main/jniLibs/arm64-v8a/`, then
   `gradlew.bat assembleArm64Release -x rustBuildArm64Release` finished in 41 s.
   `aapt dump badging` and `apksigner verify --print-certs` agree: `CN=Campaign Keeper`, SHA-256
   `9b4b266b2faa4857bbcbaf9aa457f76800f3bfeb5368e11b91ca2b3b109832a2` — the same key as 2.3.0 and
   3.0.0, so it upgrades in place.
3. `README.md`'s download table now points at the 3.0.1 files.

## Still to do
1. Paste the DeepSeek key in Settings → Other models (or set `DEEPSEEK_API_KEY`): this machine is
   pointed at DeepSeek V4 Flash but no key was available to store.
2. Optional: delete the old `installers/*3.0.0*` and `*2.3.0*` files once the owner confirms nobody
   still has them.
3. Commit only when the owner asks. End the commit message with the attribution line the owner's tools require.
4. Not tested by hand yet: start a game with the AD System on and check that the GM writes `<rpg>` lines in the format
   given in `litrpg_ad.md`. Pairing has only been exercised by the automated table tests, not with a real phone.

## Android build command (for next time)
```
$env:ANDROID_HOME="$env:LOCALAPPDATA\Android\Sdk"; $env:NDK_HOME="$env:ANDROID_HOME\ndk\android-ndk-r27c"; $env:JAVA_HOME="C:\Program Files\Android\Android Studio\jbr"
cd app; npx tauri android build --apk --target aarch64
```
Output: `app/src-tauri/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release.apk` -> copy to
`installers/Campaign Keeper 3.0.0 (Android).apk`. If it stops on the symlink error, run the jniLibs fallback above.
