import { call } from "./api";

/** What kind of device this is. A phone has no folder picker and no command-line GM engines;
 *  `web` is a friend's browser showing only the player's screen. */
export const platform = $state({ mobile: false, library: "", web: false });

export async function loadPlatform() {
  Object.assign(platform, await call("app_info"));
}
