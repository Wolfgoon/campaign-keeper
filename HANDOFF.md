# Handoff: Campaign Keeper 3.0.7 (Windows; AI Game Master, AD System, serverless multiplayer)

Repo: `C:\Users\patri\OneDrive\Documents\GitHub\campaign-keeper` (Tauri 2 + Svelte 5 + Rust). Windows, PowerShell.
Everything described here is committed and pushed to `main` (latest version: 3.0.7, Windows exe only — the
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
5. Not tested by hand yet: have the GM define a class or a species of its own (`define class …`,
   `define species …`) and reshape one with `class change …` / `species change …` at a table with two
   characters, and check the log says `CLASS CHANGE: … -> …` / `SPECIES CHANGE: … -> …` and that the new
   growth applies at the next level. Since 3.0.6 the reshape only lands at level 5 (class) and level 25
   (species, or at creation on a level 1 sheet), so give the test character experience first. The lines
   work in `ad::tests` and `game::tests`; the live table has only seen up to the 3.0.3 build, where the
   define was refused.
6. Optional: the tracked `installers/*3.0.4*`, `*3.0.5*` and `*3.0.6*` files are superseded by 3.0.7 and can
   be deleted in a commit of their own.

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

## Fix after 3.0.3: the System could not define a class (2026-11-21)

Reported from a live AD table — "the gm can not offer classes or anything". The GM had written
`Paul: define class night shift | Night Shift | ...` and got `Refused: … (unknown change 'define')`, then
told the player the app only allows the six built-in classes. It never could: the class it wanted for Paul
was the one the story had made him, and `class offer`/`class choose` both refuse a character who already
has one.

- **Cause.** `ad::World::apply_line` recognised `define`, `rename` and the crafting sums (`lens`, `crystal`,
  `impurity`, `battery`) only as the **first word** of a line. They are world lines and need no character,
  but at a table the GM names every line (that is what the 3.0.3 prompt asks for), so a name in front made
  them fall through to the per-character verbs and fail. `game::unnamed_refusal` refused an unnamed `define`
  the same way: it treated every line as a character's change.
- **Fix.** Those lines are now `World::system_line`, tried on the raw line **and again with the character's
  name taken off**, so `define …` and `Paul: define …` are the same world change. New `rpg::world_line`
  (`define`, `rename`, `lens`, `crystal`, `impurity`, `battery`) tells `game::unnamed_refusal` which lines
  belong to the world and never wait for a name. `Rules::apply_one` reads `rename` / `rename settled` past a
  name too, so `Aria: rename settled` still ends the late-rules ask.
- **More System control: `class change <class>`.** `Sheet::change_class` (verbs
  `class change|swap|replace|reclass|set`) reshapes the class a character holds: the tier they earned
  stands, the old class's skills stay learnt, the new class's growth applies from the next level, and the
  player is told in the log (`[System] CLASS CHANGE: Survivor -> Night Shift. Each level now also gives …`).
  `offer_class`'s refusal now points at it.
- **Prompt.** `prompts/litrpg_ad.md`: new **YOU ARE THE SYSTEM** section (you are the System, not a human
  referee borrowed for the evening; define, offer and reshape freely; never tell the table the app won't let
  you, and never explain an `<rpg_notice>` to them — fix the line next turn). The `class offer` bullet now
  carries a worked `define class …` and the `class change` rule, plus a bullet saying world lines need no
  name. `prompt::PROMPT_VERSION` is 4, so running chats rebuild the GM's instructions on their next turn.
- **Tidy.** `Class chosen: Forager (null).` no longer says the System's word "null" for the first tier.
- **Tests.** New: `ad::tests::{the_system_lines_work_with_a_character_named_first,
  the_system_reshapes_a_class_the_story_has_outgrown}`,
  `game::tests::the_systems_own_lines_need_no_name_at_a_table`. `cargo test --lib`: **162 passed, 0 failed,
  2 ignored**.
- **The running campaign needs nothing**: the file format is unchanged, so old `rpg.json` files load as they
  are, and the GM can now define *night shift* and shift Paul's class in the game already in progress.
- **Shipped for Windows as 3.0.4** (2026-11-21), exe only. Bumped to 3.0.4 in `app/package.json`,
  `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock` and `app/src-tauri/tauri.conf.json`.
  `cd app; npm run tauri build` gave `installers/Campaign Keeper Setup 3.0.4.exe` — 7,331,920 bytes,
  ProductVersion and FileVersion 3.0.4, SHA-256
  `33d2698c680cca1ea9e76260071564abecdbd5a65a4d487471af03c052703836`, copied from
  `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.4_x64-setup.exe`. The binary carries the fix
  (`YOU ARE THE SYSTEM`, `class change <class>`, `CLASS CHANGE`, `reshapes the one they hold` are in it).
  `dist/` rebuilt to the same `index-C1R1GU3f.js` (no UI change). `README.md`'s download table and
  `How to install.txt` point at the 3.0.4 file. (A first build, 7,330,587 bytes / `864eea20…`, was made
  before the `Aria: rename settled` change and then replaced by this one; commit 0565144's message quotes the
  first hash.)
- **Android is still 3.0.2** and does not have any of the 3.0.3/3.0.4 fixes.

## Fix after 3.0.4: a species could be evolved, never reshaped (2026-11-21)

`class change` let the System reshape a class, but a species could only ever rise a tier: `paths species`
(when `can_evolve_species`, i.e. level 25 at base) and a 1-in-10,000 feat. A character the story turned into
something else mid-campaign — bitten, reborn, remade — had nothing to write, so the GM either left the sheet
wrong or explained the app away, which is the same failure 3.0.4 fixed for classes.

- **`Sheet::change_species`**, behind `species change|swap|replace|set|become <species>` (a `species` verb now
  exists in `World::apply_line`; `species offer` is still refused with a pointer at `paths species`). The
  species tier they reached stands, the new species' `growth` applies from the next level, its `stats` apply
  at once through `true_stat`, a path-name earned through the old species is dropped, and the player is told
  `[System] SPECIES CHANGE: Human -> Night-Born. Each level now also gives HP +3, STR +2, DEX +3.`
  Undefined species are told so: `no species 'x': define it first` — `define species` already worked and now
  has a worked example in the prompt.
- **Prompt** (`litrpg_ad.md`): the **YOU ARE THE SYSTEM** line now says "reshape a class or a species", the
  `<rpg>` example carries `species change night-born` and a `define species` line, and a new bullet spells out
  that reshapes are for *what the story made them*, while tier-ups stay `paths species` + the player's
  `choose`. `prompt::PROMPT_VERSION` is 5 (it was 4 in 3.0.4), so running chats rebuild the GM's instructions
  on their next turn.
- **Tests.** New `ad::tests::the_system_reshapes_a_species_the_story_has_outgrown` — `define species` with a
  name in front, `Wren: species change night-born`, `SPECIES CHANGE` in the log, tier still 0, growth now
  `HP 3 / DEX 3` instead of human's, the sheet view showing `Night-Born`, "already their species" and an
  undefined species refused, and `species offer` still refused. `cargo test --lib`: **163 passed, 0 failed,
  2 ignored**.
- **Nothing to migrate**: same save format; a running campaign can reshape a species the moment 3.0.5 is
  installed (the GM's instructions rebuild on its next turn because of the prompt version bump).
- **Shipped for Windows as 3.0.5** (2026-11-21), exe only — 3.0.4 had already been pushed, so its binary was
  left alone rather than rebuilt under the same number. Bumped to 3.0.5 in `app/package.json`,
  `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock` and `app/src-tauri/tauri.conf.json`;
  `cd app; npm run tauri build` gave `installers/Campaign Keeper Setup 3.0.5.exe` — 7,321,957 bytes,
  ProductVersion and FileVersion 3.0.5, SHA-256
  `59c7d217e14e53026c03181a65831db86d57ce0496cf90378d22e90ed605bab0`, copied from
  `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.5_x64-setup.exe`. The binary carries it
  (`SPECIES CHANGE`, `species change night-born`, `define species night-born`, `that is already their species`,
  `YOU ARE THE SYSTEM`, `CLASS CHANGE` are in it). `dist/` rebuilt to the same `index-C1R1GU3f.js` (no UI
  change). `README.md`'s download table and `How to install.txt` point at the 3.0.5 file; the 3.0.4 file stays
  in `installers/` for anyone who already has it.
- **Android is still 3.0.2** and has none of the 3.0.3/3.0.4/3.0.5/3.0.6 fixes.

## Fix after 3.0.5: a reshape had no age to it (2026-11-21)

3.0.4 and 3.0.5 let the System reshape a class or a species at any level — a level 1 sheet could be handed
a mythic class by a single line, and `class change` on someone with no class silently *assigned* one,
skipping `class offer` and the player's `choose`. The owner asked for the level requirement the rest of the
System already uses, with creation still free for species.

- **`Sheet::can_change_class`** — they hold a class and are level 5 (`class_tier_level(0)`, where a class
  first evolves). **`Sheet::can_change_species`** — level 25 (`species_tier_level(0)`, where a species
  first evolves), *or* level 1, which is a sheet still being made: creation is the one time before 25 that
  a species may be named. Both are checked in `change_class` / `change_species`, so every caller is gated.
- Gating on the **first** tier's level rather than their current tier's is deliberate: the current tier's
  requirement runs past the level cap at the top tiers (5^7 = 78,125 for mythical, 25^4 = 390,625 for
  cataclysmic), which would have locked those characters out of ever being reshaped.
- **`class change` on a sheet with no class is now refused** — `class offer <class>`, then `class choose`.
  Without that, a level 1 sheet could be given a class outright, which no level gate can guard.
  `offer_class`'s "they already have a class" hint now says "from level 5".
- Refusals read: `a class change waits for level 5: until then a class is what their deeds earned` and
  `a species change waits for level 25: until then a species is what they were made (a new character's is
  set at creation)`. The species line fires only at levels 2–24, because 1 is creation.
- **Prompt** (`litrpg_ad.md`): the **YOU ARE THE SYSTEM** line now carries the ages ("a class from level 5,
  a species from level 25 or at creation"), both reshape bullets spell them out, and the **A NEW CHARACTER**
  section says a species defined after the sheet can still be set while they are level 1.
  `prompt::PROMPT_VERSION` is 6 (it was 5 in 3.0.5), so running chats rebuild the GM's instructions next turn.
- **Tests.** `ad::tests::the_system_reshapes_a_class_the_story_has_outgrown` now checks the no-class refusal,
  the level 1 refusal ("waits for level 5"), and that the reshape lands at 5; the species test keeps its
  level 1 creation path; new `ad::tests::a_species_change_waits_for_level_25_once_creation_is_over` puts the
  sheet at level 2 (refused, nothing changed) and at 25 (lands, tier still 0). `cargo test --lib`:
  **164 passed, 0 failed, 2 ignored**.
- **Nothing to migrate**: same save format. A character already past 5 / 25 can reshape at once; one below
  it waits, which is the point. (A species changed at level 1 in 3.0.5 stays as it is; the gate only guards
  new lines.)
- **Shipped for Windows as 3.0.6** (2026-11-21), exe only. Bumped to 3.0.6 in `app/package.json`,
  `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock` and `app/src-tauri/tauri.conf.json`;
  `cd app; npm run tauri build` gave `installers/Campaign Keeper Setup 3.0.6.exe` — 7,319,140 bytes,
  ProductVersion and FileVersion 3.0.6, SHA-256
  `01e3635da9f019e94e4c4592b9b9c7348bc5ada32ec2c4a318ece0cfb5a3ff90`, copied from
  `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.6_x64-setup.exe`. The app exe carries the
  new rules (`a class change waits for level`, `a species change waits for level`, `they have no class yet`,
  `reshapes it, from level 5`, `a class from level 5, a species from level 25 or at creation`).
  `dist/` rebuilt unchanged (`index-C1R1GU3f.js`, no UI change). `README.md`'s download table and
  `How to install.txt` point at the 3.0.6 file; 3.0.4 and 3.0.5 stay in `installers/` for anyone who has them.
- **Android is still 3.0.2** and has none of the 3.0.3/3.0.4/3.0.5/3.0.6 fixes.

## Fix after 3.0.6: amplitude you could never spend (2026-11-21)

The owner's words: "BALANCE THE AMPLITUDE A BIT YOU GET LIKE 10e100". The panel was honest — the rules that
were ported say `amplitude = INT ^ 100`, so INT 10 really did hold 10^100 in every frequency. Nothing in a
campaign can spend 10^100, and semi skill power is only `base x log10(1 + amplitude) / 2`, so the tank read
as infinity and every cast was a rounding error.

- **`DERIVED` amplitude is now `int ^ 2 * 10`** — ten times INT squared: **1,000 at INT 10**, 9,000 at INT
  30, 100,000 at INT 100. The shape is kept (capacity grows with the square of INT, so INT is still the stat
  that decides how much a frequency can hold) and the scale is the creature rule's own ten-per-point (a
  creature with no prism holds its level x 10, `CREATURE_AMPLITUDE_PER_LEVEL`).
- Why that number: the prompt already tells the GM a semi skill needs **amplitude in the 100s** to be worth
  anything. 1,000 is ten full 100-amplitude casts from a full frequency — a resource with a floor and a
  rhythm — and it keeps the shapes the tests already pin: 300 amplitude still overpowers a level 3 wolf (its
  30 remaining x 10) on a level 1 character's INT.
- **Regen has a pace now**: `rate` is untouched (his rule: `INT^2 x maximum^(1/20)`), so a full 1,000 costs
  about ten minutes of quiet, five resting, and one 100-amplitude cast is a minute's walk. Before, filling
  10^100 at ~10^7 a minute was 10^93 minutes, i.e. never.
- **Old saves are brought down, not left broken**: `Prism::fill` now clamps as well as fills — any frequency
  holding more than its top is set to the top (and `top` already accounts for overuse, the only thing that
  lowers it). `fill` is called by `spend`, `regen`, `remaining` and `sheet_view`, so a 3.0.6 save reading
  `1.00e100/1.00e100` shows `1000/1000` on the first look, with nothing edited by hand. A frequency below
  the top is left alone: fill is not a refill.
- **Prompt** (`litrpg_ad.md`): "the derived stats are enormous by design (amplitude at INT 10 is 10^100)"
  became "amplitude is capacity, not damage: ten times INT squared, so 1,000 at INT 10 and a few thousand at
  level 30 … a full one refills in about ten minutes of quiet, five resting", with speed and stamina still
  called enormous by design. `prompt::PROMPT_VERSION` is 7 (it was 6 in 3.0.6), so running chats rebuild the
  GM's instructions next turn and stop quoting 10^100.
- **Tests.** `amplitude_is_int_to_the_hundredth_exactly` became `amplitude_is_ten_times_int_squared_exactly`
  (1,000 at INT 10; the `short()` check for `1.00e100` stays, because the short form still has to carry any
  size); the regen test now expects 100 a minute from 1,000; the overuse test reads its half-maximum as 500;
  and new `a_save_holding_more_than_the_maximum_is_brought_down_to_it` puts 10^100 in every frequency, fills,
  finds 1,000, spends 600 and checks the 400 left survives a second fill. `cargo test --lib`:
  **165 passed, 0 failed, 2 ignored**.
- **Shipped for Windows as 3.0.7** (2026-11-21), exe only. Bumped to 3.0.7 in `app/package.json`,
  `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock` and `app/src-tauri/tauri.conf.json`;
  `cd app; npm run tauri build` gave `installers/Campaign Keeper Setup 3.0.7.exe` — 7,329,358 bytes,
  ProductVersion and FileVersion 3.0.7, SHA-256
  `15e80536bf36740cf195f24386b39edb2c9bda161e393e05f5ef198fa5bf5e44`, copied from
  `app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.0.7_x64-setup.exe`. The app exe carries the
  new formula and prompt (`int ^ 2 * 10`, `Amplitude is capacity, not damage`) beside the 3.0.6 gates.
  `README.md`'s download table and `How to install.txt` point at the 3.0.7 file.
- **Speed and stamina are still enormous** (`dex ^ str * 0.01` is 10^8 at DEX 10, STR 10; stamina
  `str ^ hp * 0.1` is 10^9). They are display-only — nothing in the System spends or checks them — so they
  were left as ported. Same one-line change in `DERIVED` if the owner wants them on the same footing.
- **Android is still 3.0.2** and has none of the 3.0.3–3.0.7 fixes.


