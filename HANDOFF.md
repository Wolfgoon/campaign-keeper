# Handoff: Campaign Keeper 3.0.3 (Windows; AI Game Master, AD System, serverless multiplayer)

Repo: `C:\Users\patri\OneDrive\Documents\GitHub\campaign-keeper` (Tauri 2 + Svelte 5 + Rust). Windows, PowerShell.
Everything described here is committed and pushed to `main` (latest version: 3.0.3, Windows exe only — the
Android APK is still 3.0.2). Start with **Still to do** at the bottom of the version sections.

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

## Merge with origin/main (2026-11-21)

Everything above was committed as `5fc11c1` ("yes"). `origin/main` then turned out to have three
older commits from P-Hoep (`ea9a3fc`, `878ca4c`, `4b1d5dc` "deepseeks"), so it was merged in as
`2dd9eba` and pushed to `main`.

- Conflicts (`config.rs`, `engine/openai.rs`, `scribe.rs`, `Settings.svelte`) were resolved in
  favour of this branch. Origin's separate `deepseek` engine mode (`cfg.deepseek_key`,
  `Config::resolved_deepseek_key`, `openai::{send_post, read_sse_stream, stream_deepseek,
  once_deepseek}`) is replaced here by the DeepSeek preset on Other models
  (`https://api.deepseek.com/v1`, `deepseek-v4-flash`) plus the `DEEPSEEK_API_KEY` fallback.
- `engine/mod.rs` and `app/src/lib/api.ts` were auto-merged with origin's routing into that removed
  client, so both were restored to this branch's versions; otherwise `deepseek-flash` would have
  appeared as a pickable model under the Anthropic engines.
- Kept from origin: its Linux artifacts (`Campaign Keeper 2.0.7.AppImage`, `… 2.0.7.dmg`) and the
  `/TEMPLATE_2` line in `.gitignore`.
- After resolving: `cargo test --lib` 143 passed, 0 failed; `svelte-check` 0 errors, 4 warnings.

## Still to do
1. Paste the DeepSeek key in Settings → Other models (or set `DEEPSEEK_API_KEY`): this machine is
   pointed at DeepSeek V4 Flash but no key was available to store.
2. Optional: delete the old `installers/*3.0.0*`, `*3.0.1*` and `*2.3.0*` files (they are tracked and pushed;
   removing them is a commit of its own) once the owner confirms nobody still has them.
3. Not tested by hand yet: start a game with the AD System on and check that the GM writes `<rpg>` lines in the format
   given in `litrpg_ad.md`. Pairing has only been exercised by the automated table tests, not with a real phone.
4. Not tested by hand yet: turn the rules on in a campaign that has already begun, and check that the GM asks the
   table what the world calls its words (`<world_words>`), that the answers land on the panel and in `<rpg>`
   lines, and that `rename settled` or the panel's "✔ The words are settled" button ends the asking.

## Android build command (for next time)
```
$env:ANDROID_HOME="$env:LOCALAPPDATA\Android\Sdk"; $env:NDK_HOME="$env:ANDROID_HOME\ndk\android-ndk-r27c"; $env:JAVA_HOME="C:\Program Files\Android\Android Studio\jbr"
cd app; npx tauri android build --apk --target aarch64
```
Output: `app/src-tauri/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release.apk` -> copy to
`installers/Campaign Keeper 3.0.1 (Android).apk`. If it stops on the symlink error, run the jniLibs fallback above.

## Party pages, player action buttons, one Host tab (2026-11-21)

The host and the players now work in the same UI; only host-only tools differ.

- **One Host tab.** `App.svelte`'s top bar is now `🏠 Overview · ▶ Play · 👥 Party · 🛠 Host`.
  `views/Host.svelte` (new) keeps the old host-only pages under sub-tabs: `👥 Table`, `📁 Files`,
  `✏ Scribe`, `🧠 Memory`, `🔍 Search`. `Table.svelte` no longer lives in Play's side panel.
- **A shared Party page.** `views/Party.svelte` (new): the table roster, the party's shared notes and
  the host's sheet. It reads the same state players get, so the host sees what they see.
- **A shared notes page.** `net/table.rs` gained `NOTE_FILE = "04_Quests/party_notes.md"`,
  `save_note`, `host_note`, a `note` field in every player's `table` message, and a `{ "t": "note" }`
  client message. Any seated player (browser or app) may write it, and the host can write it from
  Party; each save is broadcast to everyone. New commands: `party_notes` (read) and `party_note`
  (write). Older campaigns get the file on the first save. `TEMPLATE/04_Quests/party_notes.md` is new.
- **Players' own sheet only.** `table_for` still sends just `rules.view(character)`; a new test
  (`net::table_tests::the_party_shares_one_page_and_each_player_gets_only_their_own_sheet`) checks
  a browser player's note reaches the others and that only their own sheet crosses the wire.
  `Status.svelte` gained `only` (`null` = every sheet, a name = that one); Play, Party and
  Overview pass the host's character while `table.host_is_gm` is false, so a hosting player sees
  exactly their own sheet, not the party's.
- **Web/app player action bar.** `Client.svelte`'s composer now has `> Do` / `" Say` modes, one-tap
  `👀 Look`, `👂 Listen`, `⏳ Wait` (they fill the box rather than sending), and `👥 Party`,
  `📊 Sheet`, `📜 Notes` buttons. The side panel gained a `📜` Party notes tab. The browser gets all
  of it because it runs the same component.
- Docs: `README.md` and `How to install.txt` point at **🛠 Host → 👥 Table** and describe what the
  whole table shares.
- Verified: `cargo test --lib` 144 passed, 0 failed, 2 ignored; `npx svelte-check` 0 errors,
  4 warnings (the same four as before); `cd app; npm run build` rebuilt `dist/` (needed: `net/web.rs`
  embeds it).
- The installers in `installers/` were **not** rebuilt for this change yet: they still contain the
  previous `dist/`, so the browser page they serve has the old player screen. Rebuild both (commands
  above) to ship it.


## The world's own words, and asking for them (2026-11-21)

The AD System's vocabulary is the GM's to fit, and a world that already exists must be asked about it.

- **More words the GM may rename.** `ad::renamable()` now also covers `species`, `class` / `classes`,
  `skill` / `skills`, `path` / `paths`, `feat` / `feats`, `title` / `titles`, `condition` / `conditions`,
  `inventory` and `money`, so `rename skills -> Talents` or `rename classes -> Callings` works as well as
  `rename amplitude -> Aether`.
- **The panel prints the story's names.** `ad::panel_labels()` (used by `rpg::Rules::view`) sends the renamed
  words, keyed lowercase, with every sheet view; `Status.svelte` looks each heading up with `N()`
  (case-insensitive), so the `[ System ]` bracket, Level, Experience, the stats, Spectrums, Skills, Feats,
  Titles, Conditions, Inventory, money, the classes offered, the System log and the per-level hover all use the
  world's words. Derived stat names are relabelled too (`view()` rewrites the `derived` keys).
- **A renamed keyword still parses.** `ad::World::apply_line` runs the verb of an `<rpg>` line through
  `unlabel`, and the `paths` / `choose` / `beyond` arguments as well, so after `rename class -> Calling`,
  `Calling offer forager` means `class offer forager`.
- **Forced ask when the rules arrive late.** `Rules.terms_pending` (new; serde-default, so old `rpg.json`
  files still load) is set by `rpg_enable` when the rules are turned on in a world that has already begun
  (a character came from the files, or the GM has already replied). While it is set, `game.rs` puts the
  ruleset's ask on the player's message as `<world_words>` (`ad::TERMS_ASK` for the AD System, `rpg::TERMS_ASK`
  for Classic): the GM must ask the table what this world calls its words, end its reply there, and write the
  `rename` lines when they answer. `Rules::apply` clears the flag on a successful `rename` line and on
  `rename settled` / `words settled` (for "keep the sheet's words"); the new `rpg_terms` command does the same
  behind the host's "✔ The words are settled" button in the status panel.
- `prompts/litrpg_ad.md` and `prompts/litrpg.md`: the renamable lists and a "WHEN THE SYSTEM / RULES ARRIVES
  LATE" paragraph explain `<world_words>`. `prompt::PROMPT_VERSION` is now 2, so running chats rebuild their
  instructions on their next turn.
- Tests: `ad::tests::the_storys_words_reach_the_panel_and_the_lines` and
  `rpg::tests::the_gm_names_the_system_and_the_asking_stops` (which also checks a refused rename settles
  nothing). Verified: `cargo test --lib` 146 passed, 0 failed, 2 ignored; `npx svelte-check` 0 errors,
  4 warnings; `cd app; npm run build` rebuilt `dist/` (`index-C1R1GU3f.js`).

## Version 3.0.2: the world's own words, shipped (2026-11-21)

Bumped to **3.0.2** in `app/package.json`, `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock` and
`app/src-tauri/tauri.conf.json`; the Android CLI regenerated `app/src-tauri/gen/android/app/tauri.properties`
as `versionCode=3000002`, `versionName=3.0.2`. This is the first build that carries the section above (and the
party page / player buttons from the one before).

1. **Windows installer**: `installers/Campaign Keeper Setup 3.0.2.exe` — 7,306,837 bytes, ProductVersion and
   FileVersion 3.0.2, SHA-256
   `a72888062235ce9b24a3a235998c4c736f6026ed413b8bab30e7cc2fa63d80d1`. Built with
   `cd app; npm run tauri build` (NSIS, currentUser, 3 m 39 s), copied from
   `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.2_x64-setup.exe`.
2. **Android APK**: `installers/Campaign Keeper 3.0.2 (Android).apk` — 23,904,909 bytes, versionCode 3000002,
   versionName 3.0.2, SHA-256
   `72ec9d8b6ef699524eda11e8255fb16a45ffeb230acd39aa8458b4e02315dd8f`.
   `npx tauri android build --apk --target aarch64` stopped again at "Creation symbolic link is not allowed
   for this system" (no Developer Mode) after finishing the Rust build (`libcampaign_keeper_lib.so`,
   22,198,848 bytes, 1 m 37 s). Fallback used: that `.so` copied to
   `gen/android/app/src/main/jniLibs/arm64-v8a/`, then `gradlew.bat assembleArm64Release -x
   rustBuildArm64Release` finished in 1 m 21 s. `aapt dump badging` and `apksigner verify --print-certs`
   agree: `CN=Campaign Keeper`, SHA-256
   `9b4b266b2faa4857bbcbaf9aa457f76800f3bfeb5368e11b91ca2b3b109832a2` — the same key as 2.3.0, 3.0.0 and
   3.0.1, so it upgrades in place.
3. `README.md`'s download table and `How to install.txt` now point at the 3.0.2 files.

Verified after the bump: `cargo test --lib` 147 passed, 0 failed, 2 ignored; both installers embed the rebuilt
`dist/` (`index-C1R1GU3f.js`), so browser players served by a host on 3.0.2 get the new player screen.

## Fix after 3.0.2: experience only reached one sheet (2026-11-21)

Reported from a real table: "all the xp only goes to the host's stats."

- **Cause.** `game::apply_rpg` resolved the character for an `<rpg>` line with no `Name:` prefix as
  `s.rules.names().into_iter().next()` — the first key of the `BTreeMap`, i.e. whichever sheet sorts first
  alphabetically. Lines the GM did name (`Aria: xp +40`) always went to the right sheet, but every unnamed one
  (`xp +120`, `defeat 4 the wolf`, `money +10`) landed on that one sheet, which at a table is as often as not
  the host's character. It also applied to AD (`World`), so `defeat` and `skill xp` could pool on one sheet.
- **Fix, three parts.**
  1. `apply_rpg` now takes the turn's `p.actions` and resolves an unnamed line to **the character who acted**,
     when exactly one did (a solo turn, or one player acting in a round). A table round with several actors no
     longer guesses.
  2. With several characters and no single actor, an unnamed line is **refused** with an actionable notice:
     `Refused: xp +20 (say whose it is: Aria, Bram (or all: for everyone))`, which goes into `rpg_notices`, so
     the GM names it next turn. Nothing lands on the wrong sheet any more.
  3. New party lines: `all: …` / `everyone: …` / `party: …` apply the change to every sheet
     (`all: xp +40`, `all: money +10`, `all: item + Rations x2`). Implemented once in `rpg::Rules::apply`
     (`apply_party` → `apply_one` per sheet) so Classic and AD behave the same; `rename` is refused there.
     A per-sheet refusal doesn't undo the others, and the message says who took it and who didn't.
- **Prompts.** `prompts/litrpg.md`, `prompts/litrpg_ad.md` and `prompts/multiplayer.md` now say: with several
  characters every line starts with the character's name, unnamed lines change nothing, and `all:` is for what
  the party earned together ("a shared victory's experience belongs to everyone who fought for it").
  `prompt::PROMPT_VERSION` is 3 now, so chats already running rebuild their instructions.
- **Code touched.** `rpg.rs`: `party_line`, `Rules::apply` split into `apply_party`/`apply_one`,
  `Rules::names_character`, `Rules::say_whose` (the "no sheet for ''" refusal now lists the names);
  `ad.rs`: `World::say_whose`, same refusal; `game.rs`: `default_character`, `unnamed_refusal`,
  `apply_rpg(…, acted)` plus `&p.actions` at the call site. The Classic branch now goes through
  `Rules::apply` (was `rpg::apply_line`) so party lines work there too. Classic shows a refused line to the
  players as well as the GM (as it already did); AD keeps refusals for the System alone (item 73).
- **Tests.** New: `rpg::tests::{all_prefixes_a_party_line, names_character_sees_the_sheets_own_names,
  a_party_line_changes_every_character, a_party_line_refuses_a_rename,
  an_unnamed_line_with_two_characters_says_whose_it_should_be, a_party_line_reaches_every_status_window,
  the_system_too_asks_whose_line_it_is}` and
  `game::tests::{experience_goes_to_the_character_who_acted,
  an_unnamed_line_waits_for_a_name_when_two_characters_acted, a_named_line_still_picks_that_character,
  a_solo_game_still_takes_unnamed_lines, the_system_too_gives_experience_to_the_one_who_acted}`.
  `cargo test --lib`: **159 passed, 0 failed, 2 ignored**.
- **Shipped for Windows as 3.0.3** (2026-11-21), exe only. Bumped to 3.0.3 in `app/package.json`,
  `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock` and `app/src-tauri/tauri.conf.json`.
  `cd app; npm run tauri build` (NSIS, currentUser; 3 m 17 s Rust, then makensis) gave
  `installers/Campaign Keeper Setup 3.0.3.exe` — 7,325,143 bytes, ProductVersion and FileVersion 3.0.3,
  SHA-256 `e48dd8d8dc45c210d224a677cb614f40d5a3765e16b5295ddb55686b2c5b7f64`, copied from
  `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.3_x64-setup.exe`. The binary carries the fix
  (strings `start the line with all:`, `(or all: for everyone)`, `shared victory` are in it). `dist/` rebuilt
  to the same `index-C1R1GU3f.js` (there was no UI change).
- **Android is still 3.0.2** — `installers/Campaign Keeper 3.0.2 (Android).apk`, versionCode 3000002 — and
  does not have this fix. The next `npx tauri android build --apk --target aarch64` (jniLibs fallback if it
  stops on the symlink) takes 3.0.3 from the bumped files and regenerates
  `gen/android/app/tauri.properties` as `versionCode=3000003`.
- `README.md`'s download table and `How to install.txt` now point at the Windows 3.0.3 file; the Android rows
  stay on 3.0.2 until that APK is cut.

