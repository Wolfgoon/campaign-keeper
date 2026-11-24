# Handoff: Campaign Keeper (Windows, Android, Mac and Linux 3.1.2; AI Game Master, AD System, serverless multiplayer)

Repo: `C:\Users\patri\OneDrive\Documents\GitHub\campaign-keeper` (Tauri 2 + Svelte 5 + Rust). Windows, PowerShell.
Everything described here is committed and pushed to `main` except the 3.0.8, 3.1.0, 3.1.1 and 3.1.2 work at the
end of this file (latest version: **3.1.2 for Windows, Android, Mac and Linux**). Start with **Still to do** at the
bottom of the version sections.

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
- **Android is now 3.0.7 too** — see below.

## The Android APK for 3.0.7 (2026-11-22)

`installers/Campaign Keeper 3.0.7 (Android).apk` — 23,963,113 bytes, versionCode **3000007**, versionName
**3.0.7**, minSdkVersion 24 (Android 7+), targetSdkVersion 37, `native-code: 'arm64-v8a'`, SHA-256
`af0e6e6bc86cf5ff1568677ba395e6e7a216800dc8092ee3b2b07fb92ab41ce8`. `apksigner verify --print-certs`
gives `CN=Campaign Keeper`, SHA-256 `9b4b266b2faa4857bbcbaf9aa457f76800f3bfeb5368e11b91ca2b3b109832a2` —
the same key as 2.3.0 and 3.0.0–3.0.2, so it upgrades in place. This carries every fix from 3.0.3 to 3.0.7
(`all:` party lines and unnamed-line refusals, `define`/`class change`/`species change` with their level
gates, and amplitude `int ^ 2 * 10`) and the rebuilt `dist/` (`index-C1R1GU3f.js`).

- `cd app; npm run build` first (identical output to 3.0.7: `index-C1R1GU3f.js`, 270,543 bytes).
- `npx tauri android build --apk --target aarch64` (env vars as in the command block above) stopped again at
  "Creation symbolic link is not allowed for this system" after finishing the Rust build
  (`libcampaign_keeper_lib.so`, 22,257,056 bytes, 2 m 01 s). It did regenerate
  `gen/android/app/tauri.properties` as `versionCode=3000007`, `versionName=3.0.7`.
- Fallback: the `.so` was copied to `gen/android/app/src/main/jniLibs/arm64-v8a/`, then
  `gradlew.bat assembleArm64Release -x rustBuildArm64Release` finished in 1 m 15 s
  (161 tasks, BUILD SUCCESSFUL) and wrote `app/src-tauri/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release.apk`,
  copied to `installers/` under the versioned name. The APK's `lib/arm64-v8a/libcampaign_keeper_lib.so` is
  the fresh 22,257,056-byte build (stored uncompressed, so the size matches exactly).
- `README.md`'s download table and `How to install.txt` now point at this APK.
- Not installed from this machine: `adb devices` showed no phone attached, so the APK still needs to be
  copied to the phone (or installed with `adb install -r`) before it has been run by hand.

## 3.0.8, Android: finding a campaign again, and getting it off the phone (2026-11-22)

The phone had two campaigns and no way back into either: the welcome screen offered only **Start a new
campaign…**. What the owner saw on the device was the whole of the bug — no "Recent" list at all.

**Why.** The one and only list of stories was `cfg.recent`, which `activate()` fills in as a campaign is
opened and saves to `config.json`. But the window keeps its own snapshot of the settings, and every
`set_config` (Settings, Join a game, the table, Voices, Sound) sends that snapshot back and replaces the
whole file. A snapshot taken before a campaign was opened therefore wrote `recent: []` over the list. The
phone's `config.json` still had the player's name in it (`Wolf`), so the file itself was being written —
only the list was being lost. Nothing else on the welcome screen could reach a campaign, which is why the
two stories were unreachable rather than merely unlisted.

- **The app now lists what is on disk.** `campaign::find(&dir)` in `campaign.rs` reads the folder of
  campaigns (`/data/user/0/com.wolfgoon.campaign_keeper/campaigns` on Android), keeps every folder that
  `is_campaign` accepts, and returns each one's path, name, save count and when it was last played (the
  newest of the folder, `.keeper/chat.json` and the newest file in `08_Saves`), newest first. The
  `list_campaigns` command exposes it and `Welcome.svelte` draws **Your campaigns** from it, so a story
  shows up whether or not anything remembers opening it. The recent list stays underneath as "Also
  opened", minus anything already listed, for stories opened from elsewhere on a PC.
- **A stale window can no longer forget a story.** `set_config` now keeps the settings' own `recent` and
  `last_open` and only takes the rest from the window; the ✖ beside a recent entry calls the new
  `forget_recent(path)` instead of rewriting the list.
- **The window's copy stops going stale.** `App.svelte` re-reads `get_config` after `open_campaign` and
  `new_campaign`, so the settings it saves back are the current ones.
- **`export_campaigns` — a copy that can leave the device.** Zips every campaign (everything `.keeper/`
  holds, minus trash, backups and half-written temp files) to the Documents folder, one entry per campaign
  folder: `getExternalFilesDir(Documents)` on Android —
  `/sdcard/Android/data/com.wolfgoon.campaign_keeper/files/Documents/` — and `Documents` on Windows, as
  `Campaign Keeper <date> <time>.zip`. The welcome screen's **💾 Save a copy of every campaign (zip)**
  button runs it and toasts the path. It deliberately does *not* set `large_file(true)`: that forces ZIP64,
  and Windows Explorer and PowerShell's `Expand-Archive` then refuse the file with "A local file header is
  corrupt" (the entries read as 4,294,967,295 bytes). `cargo test --lib`: **166 passed, 0 failed, 2
  ignored** (new: `the_folder_of_campaigns_lists_what_is_there`).

The two campaigns were recovered from the phone with that button and `adb pull`:
`Super Earth` (`.keeper/chat.json` 613 bytes) and `Audacity` (`.keeper/chat.json` 111,978 bytes) — 58 files
— unzipped into `C:\Users\patri\OneDrive\Documents\Campaign Keeper\`, where the Windows app lists them too.

- Bumped to **3.0.8** in `app/package.json`, `app/src-tauri/Cargo.toml`, `app/src-tauri/Cargo.lock` and
  `app/src-tauri/tauri.conf.json`; the Android CLI regenerated `gen/android/app/tauri.properties` as
  `versionCode=3000008`, `versionName=3.0.8`. Frontend `npm run build`: `index-QAeiSslE.js`, 272.02 kB, and
  `index-CrxweYxp.css`, 11.29 kB. **The frontend is embedded in `libcampaign_keeper_lib.so`**, not in the
  APK's `assets/` — a UI change only reaches the APK through the Rust build, and `tauri-build` reruns it
  when `dist/` changes.
- **The top bar fits a phone.** `Close` used to sit off the right edge of the tab bar (it scrolled, which
  nobody discovers); the campaign's name is the heading of the Overview page, so at 760px and under the
  bar's title is hidden and the tabs are a little tighter: `Overview · Play · Party · Host · ⚙ · Close`,
  all visible on the S24 Ultra.
- `npx tauri android build --apk --target aarch64` (env vars as in the command block above) stopped at
  "Creation symbolic link is not allowed for this system" after the Rust build; the same jniLibs fallback
  finished the APK (`gradlew.bat assembleArm64Release -x rustBuildArm64Release`, BUILD SUCCESSFUL). The
  APK's `lib/arm64-v8a/libcampaign_keeper_lib.so` hashes equal to
  `src-tauri/target/aarch64-linux-android/release/libcampaign_keeper_lib.so`; the check that matters is the
  embedded frontend, which the `.so` carries (`index-QAeiSslE.js` present, the older `index-Cr09FpeL.js`
  absent).
- `installers/Campaign Keeper 3.0.8 (Android).apk` — 24,039,821 bytes, versionCode 3000008, versionName
  3.0.8, arm64-v8a only, minSdk 24 / targetSdk 37, SHA-256
  `bc81204d2ec2182107b989ba9c66f25ca0a59a5efbae5aeff7d1a3b6ef0a592d`. `apksigner verify --print-certs`:
  `CN=Campaign Keeper`, SHA-256 `9b4b266b…9832a2` — the same key as 2.3.0–3.0.7, so it installs over any of
  them with the campaigns untouched.
- **Checked on the phone** (SM-S928U1, `adb install -r`, data kept): the welcome screen lists `Super Earth`
  and `Audacity` with their save counts and "played today"; both open (tap the `Audacity` row → the
  Overview reads *Audacity*); `Close` returns to the list; the zip button wrote
  `Campaign Keeper 2026-11-22 02.51.zip` (75,610 bytes, 58 files) to
  `/sdcard/Android/data/com.wolfgoon.campaign_keeper/files/Documents/`, which `adb pull` + `Expand-Archive`
  opened without complaint. A tap lands where the accessibility dump says: `uiautomator dump` is the honest
  source of coordinates on this 450 dpi screen — the screenshots read about 6% short.
- Windows stays at 3.0.7; the desktop app gets the same two list fixes and the export button whenever its
  next exe is built from these sources.
- **Still to do:** commit and push `installers/Campaign Keeper 3.0.8 (Android).apk`, `README.md`,
  `HANDOFF.md` and the source changes — the README's raw link 404s until they are on `main`. The phone's
  `Documents` folder still holds the first test zip (`Campaign Keeper 2026-11-22 02.49.zip`), which is the
  ZIP64 one Windows can't open; it is inside `Android/data` and harmless, and deleting it needs the app or
  root.


## 3.1.0: the Scribe on any model, and a screen that fits a phone (2026-11-22)

**The Scribe only ran on two of the four engines.** `scribe::run` matched `api` (Anthropic key) and the
two desktop programs (Claude Code, Antigravity) and refused everything else. `Config::load` turns
`claude_code`/`antigravity` into `other` on a phone, because neither program runs there — so on Android
the Scribe could only ever answer *"it needs Claude Code, Antigravity, or an Anthropic API key"*. The one
engine a phone actually uses was the one it refused.

- **`scribe::run_other`** (`app/src-tauri/src/scribe.rs`): the same five tools and the same 40-round tool
  loop, over `chat/completions` with `tools` and `tool_choice: "auto"`. `tools_other()` converts
  `tools()` to the OpenAI function shape (`{type: "function", function: {name, description, parameters}}`).
  The assistant message is echoed back with its `tool_calls`; each result returns as a `role: "tool"`
  message keyed by `tool_call_id`; a failed tool answers `Error: …` so the model can correct itself.
  Tool arguments are accepted as a JSON string or as an object (some services send one, some the other).
  Nothing touches the disk: proposals, `apply` and `restore` are unchanged, so the review screen and the
  backups behave exactly as with the other engines.
- **`openai::chat`** (`engine/openai.rs`): one non-streaming request, retried once without
  `max_tokens`/`tool_choice` when a service answers 400/422 — the same fallback `stream` already uses.
- `Scribe.svelte` names the model it will use (*"Edits are made by gemini-flash-latest."*), says so on a
  phone, and disables **✏ Run the Scribe** with an explanation when the chosen engine has no key or model
  (`brain` is a `$derived.by`). `Host.svelte` passes `cfg` down. Settings' phone note now says the Scribe
  needs a model that can use tools (Gemini, DeepSeek, OpenRouter…).
- Tests: `an_other_service_edits_through_tool_calls_and_writes_nothing_yet` (a small TcpListener answers
  like an OpenAI-compatible service: first a `create_file` tool call, then a summary — the proposal appears
  and the disk stays untouched), `the_tools_travel_to_other_services_as_functions`, and
  `other_models_ask_for_a_model_before_any_network_call` (the "choose a model" check happens before any
  request, so it runs offline). `cargo test --lib`: **169 passed, 0 failed, 2 ignored**. `svelte-check`:
  0 errors (the same 4 warnings as before, in `Client.svelte`).

**Text size, not page zoom.** Appearance's slider set `documentElement.style.zoom`. Zoom scales the page
but leaves the layout's idea of its own width alone: media queries keep matching the unzoomed width, so a
phone's breakpoints move, `position: fixed` panels (the side panel, the map, the activity sheet) land in
the wrong place, and the result is the "wonky" scaling the owner reported. `App.svelte`'s `applyTheme()`
now sets `--ui-scale` (clamped 0.8–1.6) and `app.css` turns it into the root font size —
`:root { font-size: calc(16px * var(--ui-scale, 1)) }` — with every type size and the key paddings moved
to rem. The slider is labelled **Text size**.

**A phone bar that fits.** Under 520px the four tabs drop their words (🏠 ▶ 👥 🛠, with `title` and
`aria-label` keeping the names for screen readers and long-press), the flexible spacer stays so **⚙** and
**Close** sit at the right edge where the thumb is, and the bar no longer has to scroll. Buttons get a
40px minimum height (the composer's `.small` chips 32px), `input`/`textarea` stay at 16px under the text
scale, list rows and tabs are taller, and a campaign file's own `#`/`##` headings inside a card, the story
or the reader are held at body size on a phone instead of rendering as chapter titles (the Overview's
"Active Quests" heading was the worst of them).
- 3.1.0 in `app/package.json`, `app/src-tauri/Cargo.toml`, `Cargo.lock`, `tauri.conf.json`; the Android CLI
  regenerated `gen/android/app/tauri.properties` as `versionCode=3001000`, `versionName=3.1.0`.
- Built with the usual two steps: `npx tauri android build --apk --target aarch64` stopped at "Creation
  symbolic link is not allowed for this system" after the Rust build
  (`libcampaign_keeper_lib.so`, 22,292,640 bytes), then the fresh `.so` was copied into
  `gen/android/app/src/main/jniLibs/arm64-v8a/` and
  `gradlew.bat assembleArm64Release -x rustBuildArm64Release` finished (BUILD SUCCESSFUL in 47 s).
  Frontend: `index-DgDgn3fz.js` (273,648 bytes) and `index-DSNI4G0s.css` (11,912 bytes), carried inside
  the `.so` — the packaged `.so` is byte-identical (SHA-256 `680cbbe1…e431`) to the build output. (The
  mock-service test was written after the APK was cut; it sits behind `#[cfg(test)]`, so the shipped code
  is unchanged.)
- `installers/Campaign Keeper 3.1.0 (Android).apk` — 23,998,701 bytes, versionCode 3001000, versionName
  3.1.0, arm64-v8a, minSdk 24 / targetSdk 37, SHA-256
  `67c96e08b499eae6fa0a8521d357c7bfcb9789b8097d00a695322dae90ad7d65`. `apksigner verify --print-certs`:
  `CN=Campaign Keeper`, SHA-256 `9b4b266b…9832a2` — the same key as 2.3.0–3.0.8, so it installs over any of
  them with the campaigns untouched. `zipalign -c -v 4`: verified. Installed on the phone with
  `adb install -r` (data kept); `dumpsys package` reports 3.1.0 / 3001000.
- Windows stays at 3.0.7; the next exe built from these sources is 3.1.0 and picks up the Scribe's new
  engine, the text scale and the phone bar.
- **Checked on the phone** (SM-S928U1, `adb install -r`, data kept, `dumpsys package` → 3.1.0 / 3001000):
  - The bar is `🏠 ▶ 👥 🛠 ……… ⚙ Close` — icons only on the left, `Close` at the right edge at
    x 905–1065 of 1080, no scrolling. Host's own tabs (`👥 Table 📁 Files ✏ Scribe 🧠 Memory 🔍 Search`)
    all fit on one line (they end at x 928) and are 112 px tall.
  - Welcome rows are 104 px tall (were 95); a campaign's `#`/`##` headings inside a card now sit at body
    size instead of rendering as chapter titles.
  - **The Scribe ran on the phone.** `Edits are made by deepseek-v4-flash. This works on the phone: it uses
    the model from Settings, not a program on a computer.` On `Super Earth` it answered *"I haven't changed
    anything, because there's nothing to change it from."* (the campaign holds only template files); on
    `Audacity` (28 files, 112 KB chat) it proposed **13 changes** — `01_Character/executor.md`,
    `00_GM/settings.md`, `02_World/…`, `03_NPCs/red_ants.md` and `the_boy_at_the_log.md` (NEW),
    `06_Sessions/day1_the_fallen_log.md` (NEW), `07_Maps/map.md`, `05_Memory/pinned_memory.md`, … — with
    a per-file diff under **Show changes**, and a closing "Unsure / left alone" list. **Discard all** cleared
    it; nothing was written, which is the rule the tests pin down.
  - Test images and `ui*.xml` were pulled off `/sdcard` afterwards.
- **Still to do:** commit and push (source, `installers/Campaign Keeper 3.1.0 (Android).apk`, README,
  HANDOFF, How to install) — the README's raw link 404s until it is on `main`. Everything from 3.0.8 is
  still uncommitted too.

---

## 3.1.1 — patience on a slow line, files that scroll on a phone, an OOC button

Three complaints, three fixes. Everything else is 3.1.0.

**Connections now wait a minute.** Every wait a person sits through was short enough to call a live
host dead: joining a table gave up after **6 s** (and the handshake after 20 s), a dropped client
rejoined for only ~32 s, a service's model list timed out at 20 s, and the host dropped a silent
player after 50 s while the client gave up at 45 s.
- `net/mod.rs` has `REACH` (60 s): used for the join's connect, the client's handshake read, the
  host's handshake read and a browser's request head (`net/client.rs`, `net/table.rs`, `net/web.rs`).
  The client's "didn't answer" message now says it waited a minute.
- `client::QUIET` 45 s → **120 s**, and the host's `table::quiet()` 50 s → **180 s**: the host now
  outlasts the client, so a phone that slept for a minute finds its seat still there instead of the
  round having moved on without it. Heartbeats are still every 15 s.
- `Client.svelte`'s reconnect loop is now a **three-minute budget** (not 8 tries ≈ 32 s); the message
  when it truly gives up says the host may have closed the table.
- `openai::list_models` 20 s → 60 s, so **List models** on a phone's data plan stops failing.
- Model replies were already generous (1800 s for Anthropic and OpenAI-compatible, 900 s for the
  Scribe) and are untouched.

**Files scroll on a phone.** The page's root never scrolls (`body { overflow: hidden }`), so the reader
and the file list carry their own scrolling. `.split` and `.editor` were flex items with
`min-height: auto`: on a phone (where `.split` turns into a column) the reader refused to shrink below
its content, grew past the window, and nothing moved — only the editor's textarea, which scrolls
inside itself, worked. That is why a long file could only be read by pressing **✏ Edit**. `.split`,
`.editor`, `.reader` and `.tree .list` now all carry `min-height: 0` (with `overflow: hidden` on the
editor), so the phone's reader scrolls like the desktop one; `-webkit-overflow-scrolling: touch` and
`overscroll-behavior: contain` keep the momentum inside the pane.




**An OOC button (all engines, every campaign).** The composer's mode row gained **`? OOC`** next to
`> Do`, `" Say` and `~ Story`, and typing a message that starts with `?` does the same thing:
- The turn goes to the GM as an out-of-character question. `game.rs` has `is_ooc()` (a leading `?`,
  or `/ooc`), sends an `<out_of_character>` block with the question ("no narration, no time passing,
  no checks… what has already happened is canon even where it disagrees with the files: never change,
  soften or rewrite it; name a contradiction and let the player decide"), skips the referee, the
  status window, the world-words ask and the whole change path — no `<rpg>`, no save block, no
  activity, no memory filing — so an OOC answer cannot touch a sheet, a file or what the GM remembers.
  Memory *recall* still runs: these questions are usually about what's true.
- While the mode is on, three one-tap asks fill the box (they never send by themselves):
  **✔ Confirm** ("what has definitely happened, what we know for certain, what's still unclear"),
  **🔍 More detail** ("what I can see, hear, smell and touch… concrete and specific, consistent with
  what is already written") and **🧷 No retcons** ("check this chat and the files for contradictions;
  nothing may be changed, softened or rewritten; tell me plainly instead of narrating it away").
  The placeholder changes with the mode, and the mode's tooltip explains itself.
- Both entries carry `"ooc": true` (`lib.rs`'s `entry_view` exposes it): `Entry.svelte` shows the
  question as a dashed **OOC** box and the answer as a plain panel with a gold rule rather than story
  prose. An OOC reply is neither spoken aloud nor scored for music, and `Play.svelte` sends OOC
  straight to the GM even while a table is open — a question is not a move in the round.
- `prompts/running_inside.md` gained an **OUT OF CHARACTER** section with the same rules, and
  `PROMPT_VERSION` went 7 → 8, so chats already running pick it up on their next turn.
- Tests: `a_leading_question_mark_asks_out_of_character`,
  `an_out_of_character_answer_leaves_the_story_alone`, and
  `the_instructions_cover_questions_asked_out_of_character`. `cargo test --lib`: **172 passed, 0
  failed, 2 ignored**. `svelte-check`: 0 errors, the same 4 warnings in `Client.svelte`.

**Built and installed.** 3.1.1 in `app/package.json`, `app/src-tauri/Cargo.toml`, `Cargo.lock` and
`tauri.conf.json`; the CLI wrote `gen/android/app/tauri.properties` as `versionCode=3001001`,
`versionName=3.1.1`. The usual two steps: `npx tauri android build --apk --target aarch64` stopped at
"Creation symbolic link is not allowed for this system" after the Rust build
(`libcampaign_keeper_lib.so`, 22,285,248 bytes), the fresh `.so` was copied into
`gen/android/app/src/main/jniLibs/arm64-v8a/`, and
`gradlew.bat assembleArm64Release -x rustBuildArm64Release` finished (BUILD SUCCESSFUL in 1 m 5 s).
`local.properties` was missing, so gradle needed `ANDROID_HOME=C:\Users\patri\AppData\Local\Android\Sdk`
set in the shell.
- Also fixed while checking it: a file's path in the editor's header ran on *under* the Edit/Save/Revert
  buttons on a phone (the path shrank to nothing and spilled). It is now `.editor .path` with an
  ellipsis, and under 760px the path takes the first line with the five buttons wrapping beneath it, so
  the header reads `00_GM/settings.md` on one line and the buttons on the next.
- `installers/Campaign Keeper 3.1.1 (Android).apk` — 23,991,305 bytes, versionCode 3001001,
  versionName 3.1.1, arm64-v8a, minSdk 24 / targetSdk 37, SHA-256
  `19107ba32117ca0de476d9caf7a2339654345450c65bf25fd58273964c781e90` (the second build, with the header
  fix; the first was `b8068a0c…b5441`). `apksigner verify --print-certs`: `CN=Campaign Keeper`, SHA-256
  `9b4b266b…9832a2` — the same key as 2.3.0–3.1.0, so it installs over any of them with the campaigns
  untouched. `zipalign -c -v 4`: verified. Installed on the phone with `adb install -r` (data kept,
  `lastUpdateTime` 2026-11-22 12:12:18); `dumpsys package` reports 3.1.1 / 3001001.

**Windows, same day, from the same sources.** `cd app; npx tauri build` (vite → `index-Cs8WYg5H.css`
12.66 kB, `index-DKRkUhEm.js` 275.46 kB; release cargo 3 m 47 s) made
`app/src-tauri/target/release/bundle/nsis/Campaign Keeper_3.1.1_x64-setup.exe`, copied over
`installers/Campaign Keeper Setup 3.1.1.exe` — 7,366,221 bytes, FileVersion and ProductVersion **3.1.1**,
ProductName "Campaign Keeper", SHA-256
`4b509c47bd0de870f2e9cfec55baf69e2774e0dde08a42b4a5e06e96a64c7c00`. NSIS, `installMode: currentUser`
(no administrator), unsigned like every earlier exe, so SmartScreen says "Windows protected your PC" once
per computer; it upgrades any 2.x/3.0.x/3.1.x install in place and leaves `Documents\Campaign Keeper`
alone.
- **Compatible with everything else at a table.** `net::PROTOCOL` is still **3** and 3.0.8–3.1.1 changed no
  wire field, so a 3.1.1 Windows host, 3.0.x/3.1.x phones and browser tabs all still join each other —
  `the_web_page_speaks_the_hosts_protocol` and the rest of the table tests enforce it. The campaign files
  and saves have no schema change either, so 3.1.1 opens folders written by 2.x and 3.0.x, and older apps
  open folders 3.1.1 writes.
- **Checked on the desktop** (the freshly built exe, window "Campaign Keeper", ~29 MB): the welcome page
  lists both campaigns plus "Also opened"; Audacity opened and Overview drew; **Host → Files** showed
  `00_GM/settings.md` with the path on one line and Edit/Save/Revert/Rename…/Trash… to its right (the
  760px wrap is phone-only) and the reader scrolled on its own scrollbar; **Play** showed the `? OOC`
  chip, and clicking it brought the "Out of character: ✔ Confirm · 🔍 More detail · 🧷 No retcons" row
  and the OOC placeholder, with **✔ Confirm** filling the box and sending nothing. Closed cleanly.
- Driving the desktop from PowerShell works with `SetCursorPos` + `mouse_event(2/4)` in **physical** pixels
  measured from a `CopyFromScreen` of `GetWindowRect` — but a click in the 2px gap between two chips does
  nothing at all, silently (that cost one detour).
- `README.md`'s download row and `How to install.txt` now say **3.1.1** for Windows too.
- **Checked on the phone** (SM-S928U1, `adb install -r` twice, data kept, `dumpsys` → 3.1.1 / 3001001):
  - **Files now scroll in read mode.** The tree scrolled from `00_GM/…` down to
    `02_WORLD/ITEMS_AND_LORE`, and `00_GM/settings.md` opened, scrolled from its first heading to the
    play-style list further down, and never needed **✏ Edit**. The header shows the path on its own
    line with Edit/Save/Revert/Rename…/Trash… under it.
  - **The OOC button works end to end.** Tapping **`? OOC`** lit the chip gold, the placeholder became
    *"Ask out of character: confirm, more detail, no retcons"*, and the three asks appeared under it
    (**✔ Confirm · 🔍 More detail · 🧷 No retcons**). **✔ Confirm** filled the box without sending;
    **Send** got an answer in under 25 s from `deepseek-v4-flash`, drawn as a plain panel with a gold
    rule (not story prose): what had definitely happened, "Nobody knows yet", one contradiction
    flagged for the player to settle (the status window said the bite was on the *shin*, the canon
    reading is the ankle actuator; the sheet's XP 175/437 beat the notebook's 60/437), and a closing
    *"The story is exactly where it was…"*. No story was advanced and no checks were rolled.
  - Driving the page from a PC: **`uiautomator dump` bounds are the exact touch coordinates**, while
    positions read off a 1080×2340 screenshot are ~6% low (multiply by ≈1.06). The app had to be
    relaunched after each `adb install -r`, and a locked phone shows a black screenshot until
    `adb shell input keyevent 224` + the PIN.
- `README.md`'s download row and `How to install.txt` point at `Campaign Keeper 3.1.1 (Android).apk` and
  `Campaign Keeper Setup 3.1.1.exe`. Only Mac (1.0.5) and Linux (1.0.5) are still behind.
- **Still to do:** commit and push — 3.0.8, 3.1.0, 3.1.1 and 3.1.2 are all uncommitted (the five new APKs
  and `Campaign Keeper Setup 3.1.1.exe` / `3.1.2.exe` are untracked), and the README's raw links 404 until
  they are on `main`. Then `installers/Campaign Keeper Setup 3.0.7.exe` and the older APKs can be deleted.

## 3.1.2 — the Google sign-in instructions, and an answer for Android (2026-11-22)

Words only, plus the two binaries built from them. No Rust behaviour changed, no wire change
(`net::PROTOCOL` is still **3**), no schema change: 3.1.2 tables with 3.0.x/3.1.x phones and browser tabs.

**Why.** The "Google sign-in setup (advanced)" block under Settings → Account said "make a client in the
Google Cloud console" and left the reader to find the other five screens. The console has since been
rebuilt around **Google Auth Platform**, which hides Branding / Data Access / Audience / Clients behind a
project that has none yet, so the old one-line instruction led nowhere. The questions that actually cost
an afternoon, in the order they come up:

- The **client type** must be **TVs and Limited Input devices**. Device flow refuses a Desktop app, Web
  application or **Android** client — that is the `401` / `invalid_client`. An Android client's *package
  name and SHA-1* are for the Android sign-in SDK, which this app never uses: the same device-flow client
  is pasted into the phone's own Settings and works on Windows, Linux and Android alike.
- The scope `https://www.googleapis.com/auth/drive.file` has to be added under **Data Access** by hand,
  or the device page answers `invalid_scope`.
- A consent screen left in **Testing** expires every refresh token after **7 days** ("Your Google sign-in
  has expired") and only lets listed **Test users** in ("Access blocked"). **Publish app** ends both; an
  unverified public app keeps the one-tap "Google hasn't verified this app" warning, which is fine.

**Three places, same walkthrough.**
1. `app/src/views/Settings.svelte` — the `<details>` block is now six numbered console steps (Drive API →
   Branding / External → Data Access with the exact scope string → Test users → **TVs and Limited Input
   devices** → paste and **Save and check**), two buttons that open the Drive API enable page and the Auth
   Platform (`@tauri-apps/plugin-opener`'s `openUrl`, already a dependency), the 7-day/Testing warning, the
   "that's the 401" note with the Android sentence, and a closing line about `google.com/device` and the
   `drive.file` folder.
2. `app/README.md` — the same six steps for developers, the `google_oauth.json` shape, a Testing vs.
   published paragraph, and a **"When sign-in doesn't work"** table mapping each message the app can print
   to its cause (missing client, `invalid_client`, `invalid_scope`, Drive API disabled, `insufficientPermissions`
   after adding the scope later, 7-day expiry, an expired device code, "Access blocked").
3. `How to install.txt` — a short player-facing **"Keeping stories in your Google Drive (optional)"**
   section: what sign-in does, that nothing uploads until you sign in, that the client is a ten-minute
   one-off, and where the steps live.

The Rust side already helped: `account::refused` appends the client-type fix to any 400/401 from the
device endpoint (and says an Android client's package name and SHA-1 don't apply), so the phone shows the
same instruction the docs do.

**Built.** `npx tauri build` → `Campaign Keeper_3.1.2_x64-setup.exe`, copied over
`installers/Campaign Keeper Setup 3.1.2.exe` — 7,348,114 bytes, FileVersion and ProductVersion **3.1.2**,
ProductName "Campaign Keeper", SHA-256
`27d8fcf82b3bf7ffccca0cbd02ca9ac2232cbefd71072ca4c2a6259042bd9a17`.
Android: the usual two steps — `npx tauri android build --apk --target aarch64` stopped again at
"Creation symbolic link is not allowed for this system" after the Rust build
(`libcampaign_keeper_lib.so`, 22,296,960 bytes), the fresh `.so` was copied into
`gen/android/app/src/main/jniLibs/arm64-v8a/`, and `gradlew.bat assembleArm64Release -x rustBuildArm64Release`
reported BUILD SUCCESSFUL with `packageArm64Release` **UP-TO-DATE** — the same `.so`, the same APK.
`installers/Campaign Keeper 3.1.2 (Android).apk` — 24,003,021 bytes, versionCode 3001002, versionName 3.1.2,
arm64-v8a, minSdk 24 / targetSdk 37, package `com.wolfgoon.campaign_keeper`, SHA-256
`47040c979adab7efe2c8dad67059fa618eace2edc20a73935d13e4b320d3c1b6`. `apksigner verify --print-certs`:
`CN=Campaign Keeper`, SHA-256 `9b4b266b…9832a2`, **SHA-1 `c0673e93…3b3a64`** — the same key as 2.3.0–3.1.1.
Installed with `adb install -r` (data kept, `lastUpdateTime` 2026-11-22 14:05:16), `dumpsys package`
reports 3.1.2 / 3001002, and it launched.
- Both binaries were checked for the words themselves, not assumed: the walkthrough string
  (`TVs and Limited Input`, `google.com/device`, `Google sign-in setup`) is present in
  `app/src-tauri/target/release/campaign-keeper.exe` and in the Android `.so` — a plain byte search with
  ISO-8859-1, since the frontend is embedded in the binary.
- `cd app; npm run check`: **0 errors**, the same 4 warnings in `Client.svelte`. No Rust change, so
  `cargo test` was not rerun.
- Also remember: the Android build deletes `jniLibs/arm64-v8a/libcampaign_keeper_lib.so` when the symlink
  step fails, so the `.so` must be copied in before `gradlew` (it fails otherwise with a missing file, not
  a symlink error).
- **Still to do:** commit and push; delete `installers/Campaign Keeper Setup 3.0.7.exe` and the superseded APKs
  once 3.0.8–3.1.2 are on `main`.

## 3.1.3: the picture map (Windows and Android)
- New **Map** tab (`views/MapTab.svelte`, `views/TileMap.svelte`): a 64x64 grid of 64x64 pixel-art tiles with tokens,
  a "📍 place" banner, a "what's happening" line, and a 🔔 list of the GM's non-visual points. Players at a table get
  it in their Map side panel through the table snapshot (`"tilemap"`).
- `tilemap.rs`: after every non-OOC reply, `engine::helper` (Haiku / Gemini Flash) gets the map and the reply and answers
  with commands (`new`, `area`, `fill`, `line`, `circle`, `put`, `token`, `move`, `remove`, `label`, `now`, `note`).
  Prompt: `prompts/tilemap.md`. Each area is kept apart in `.keeper/tilemaps/`. Setting: `tilemap_enabled` (on).
  Not covered by /undo.
- `tileart.rs`: every tile and token kind (28 pieces) is drawn by a model for each campaign, styled after its
  world overview: `art::paint` when an image service is set up (cropped square), otherwise an SVG from the helper.
  Both go through `pixel` k-means (16 colours, 64px). Kept in `.keeper/tileset/`, and the 🎨 button redraws it all.
- Named tokens get their own sprite once (`.keeper/tileset/t_<slug>.png`, from their campaign file), shared with players.
  The table sends the tile map only when `tilemap::stamp` changes, and syncs on `tilemap_done` (once per batch).
- **Minigames** (`views/Minigame.svelte`): lockpick, dice, combo, choose, haggle, sequence, aim, arrange, memory.
  `tags.rs` gathers `- word: value` lines into `lists`. Fixed: `.activity textarea` covered the whole crafting tab.
- **OOC bookkeeping**: `?` replies may apply `<rpg>` lines and see the status window (📒 Books preset).
- **Ledger** (`ledger.rs`, `views/Ledger.svelte`, Host → 📒 Ledger): a status-only chat with the main model that proposes
  `<rpg>` lines; Apply/Discard; the GM gets an rpg_notice. Auto-check every 3 exchanges (`ledger_auto`), shown in
  `LedgerDock.svelte` (bottom-left of Play).
- Conditions match by name, ignoring brackets (`rpg::cond_key`, `find_condition`); a miss is refused instead of a false
  "Cleared". The sheet shows bracketed detail on hover / tap.
- **AD clock**: an exchange is 10 seconds (`World::tick`) unless the reply has `rest`/`regen`; skips over 10 minutes need
  `| reason`.
- Welcome screen: two columns on computers (actions left, campaign cards right).

## 3.1.2 for Mac and Linux, the last two that were behind (2026-11-22)

Both were stuck at 1.0.5 while Windows and Android went to 3.1.2. They are now built from the same sources as
the 3.1.2 exe and APK, so a table can mix every platform again (`net::PROTOCOL` is still **3**, no schema
change, nothing to migrate).

**Mac — `installers/Campaign Keeper 3.1.2.dmg`** — 18,960,717 bytes, SHA-256
`9341b6246611e6cc15f01788a44f78ea06efe93b680d37e7ed796678fe681333`, built on the Mac with
`cd app; npx tauri build --target universal-apple-darwin --bundles dmg` and copied from
`app/src-tauri/target/release/bundle/dmg/Campaign Keeper_3.1.2_universal.dmg`. Checked after mounting it:
`lipo -archs` says `x86_64 arm64` (one file for Apple Silicon and Intel), `CFBundleShortVersionString` is
**3.1.2**, the identifier is `com.wolfgoon.campaign-keeper`, and `LSMinimumSystemVersion` is **11.0**. A copy was
dragged out and launched: the window opened and the app stayed up (Settings found `claude` at
`/usr/local/bin/claude` in the first build of the day), and it quit without a crash. Unsigned like the exe, so the
first open needs **Done** → System Settings → Privacy & Security → **Open Anyway** (already what the docs say).
`bundle.macOS.minimumSystemVersion` is new in `app/src-tauri/tauri.conf.json`: Tauri's default is 10.13, which
would have let a Catalina Mac install a build the README only promises from 11.0 up, so the DMG above was rebuilt
after setting it (the first 3.1.2 dmg, `3df72c36…a63e6`, declared 10.13 and is superseded).

**Linux / Steam Deck — `installers/Campaign Keeper 3.1.2 (Linux & Steam Deck).AppImage`** — 88,721,912 bytes,
SHA-256 `75a5d9b0fca29f980ddcfe116a3d8cfbeb47fde2d4cf67b5e37b89f69ae96434`, x86-64 (the first one built for
this release was `9a1b22a6…2451a56a` — same size, same BuildID `ef655b05…`, different squashfs timestamps;
the `75a5d9b0…` file is the one shipped).

- **It cannot be built on this Mac.** A Tauri AppImage is built by linuxdeploy out of the *host's* own GTK and
  WebKit, so it needs a Linux machine; cross-compiling it is not a thing. It was built on GitHub's runners
  instead, with the new `.github/workflows/linux.yml` (Actions → **Linux build** → **Run workflow**), which
  pins `ubuntu-22.04` and not `ubuntu-latest` on purpose: 22.04's glibc 2.35 is old enough for every SteamOS 3.x,
  Debian 12 and Ubuntu 22.04+, while a 24.04 build would refuse to start on all of them.
- **Nothing has to be installed on the Deck.** Tauri's bundler copies the whole stack into the file, and that was
  checked rather than assumed: unpacked, `usr/lib` holds `libwebkit2gtk-4.1.so.0`,
  `libjavascriptcoregtk-4.1.so.0`, `libsoup-3.0.so.0`, `libgtk-3.so.0`, GLib/GIO, Pango, Cairo, librsvg, the
  gdk-pixbuf loaders and the input modules, and `ldd usr/bin/campaign-keeper` resolves every one of them to
  `../lib`, leaving only glibc, libgcc, libm and the Mesa GL libraries to the system. (The old 2.0.7 AppImage
  bundles its stack the same way, which is why it was 103 MB.)
- **It was started with no webkit2gtk on the machine**, which is the Deck's situation: on a clean `ubuntu-22.04`
  runner with `libfuse2`, Xvfb and Mesa (`libegl1 libgl1 libgles2 libgbm1 libglx-mesa0 libgl1-mesa-dri`) and
  **no** `libwebkit2gtk-4.1-0`, the AppImage mounted, opened a window titled `Campaign Keeper` within 2 s and
  drew the welcome screen at 1280x800 (the Deck's own resolution) with the icon, the three buttons and Settings —
  the only output on the console was the usual `AT-SPI` accessibility warning. Reproduced twice.
- **The two failures on the way there are worth knowing:** without Mesa the app dies at once with
  `error while loading shared libraries: libEGL.so.1`; with `libegl1`/`libgl1` but no `libgles2` the window
  appears and stays blank, and the log says `Couldn't open libGLESv2.so.2`. SteamOS ships all of Mesa for Steam
  itself, so a Deck has them, but a bare container (or a distro with no GL) needs `libegl1 libgl1 libgles2`.
  If a future build ever *does* need system WebKitGTK, SteamOS 3.7's `extra` repo has `webkit2gtk-4.1` and
  `fuse2`, so it would be `sudo steamos-readonly disable && sudo pacman -S --needed webkit2gtk-4.1 fuse2`
  (the read-only switch resets on every SteamOS update, which is the argument for keeping it bundled).
- **How it was built, and the tidying.** Because the build had to happen on a runner but the answer was needed
  here, a temporary branch `linux-build` carried a throwaway workflow that built the AppImage, committed it back
  to that branch and then (second pass) unpacked it, `ldd`-ed it, ran it under Xvfb and took the screenshot. The
  branch and both temporary workflows are deleted; what stays is `linux.yml` (manual, artifact only) and
  `app/build-linux.sh` (the same build on a Linux box or on the Deck itself in Desktop Mode). Both now write
  `installers/Campaign Keeper <version> (Linux & Steam Deck).AppImage`, the name the shipped file has.
- **Why the file comes from the runner and not from this Mac.** `git push` of the 88 MB AppImage from here dies
  after a minute with `RPC failed; HTTP 408 curl 22 The requested URL returned error: 408` (19 MB went up in 37 s,
  so it is the size of the upload, not the credentials; there are no SSH keys on this machine either, and raising
  `http.postBuffer`/`lowSpeedTime` changed nothing). So the release file was produced by a second temporary
  workflow on a throwaway `push-test` branch: build → commit the AppImage back → a second job unpacked *that* file,
  ran it with no webkit2gtk and photographed it — which is where the numbers above come from. The blob was then
  `git reset --hard` into `main`, so publishing it only had to carry trees and commits. To do it again: push the
  docs and the other files first, let the runner add the AppImage, then reset `main` onto that branch.
- **Docs.** `README.md`'s table and install list and `How to install.txt` now name the dmg and the AppImage for
  3.1.2, say the Mac file is universal, and describe the AppImage as one file that needs nothing installed
  (right-click → Properties → Permissions → **Is executable**, or `chmod +x`; `Games → Add a Non-Steam Game`
  for Steam; `--appimage-extract-and-run` if a system without FUSE 2 won't mount it).
- **Still to do:** run it on the Deck itself — Desktop Mode, tick **Is executable**, double-click — which is the
  one check a Linux runner can't make (Game Mode needs it added as a non-Steam game first, and a mouse and
  keyboard or the on-screen keyboard). After that, `installers/Campaign Keeper 1.0.5.dmg`,
  `Campaign Keeper 1.0.5 (Linux & Steam Deck).tar.gz` and `Campaign Keeper 2.0.7.*` are superseded and can be
  deleted in a commit of their own.

## 3.2.0: built-in voices, automatic Claude Code trust, drawn map tiles

- **Read aloud ships its own voice engine** (`mespeak`, eSpeak in JS, in `app/src/lib/voice.svelte.ts`), so it no
  longer depends on the computer's voices. The narrator is one flat voice (SSML `prosody range="x-low"`, mid pitch,
  no gendered variant). Characters get one of ten fixed voices: Abel, Bram, Cass, Dov, Edge (male) and Fay, Gwen,
  Hana, Isla, Juno (female). The list is in `running_inside.md`, so the GM casts with `voice="Gwen"`. The cast is
  kept in localStorage `ck-cast2`, and the Voices panel can change and preview them. Playback goes through one
  AudioContext queue, so Stop really stops.
- **Claude Code never asks "trust this folder?"**: `claude_code::trust(dir)` sets `hasTrustDialogAccepted` in
  `~/.claude.json` (forward-slash keys) for every folder the app runs it in, including the sign-in terminal, which
  now opens in the app's own data folder.
- **Map tiles are SVG drawings only.** The image service is no longer used for them. Natural ground gets 4
  drawings (`grass`, `grass~1`…`~3`), each told to differ, and the page shows each in 4 orientations. A
  `drawn-v2` marker clears old painted tilesets once.
