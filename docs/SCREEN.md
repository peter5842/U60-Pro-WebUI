# Geek screen (LCD)

`zte-screen` replaces the stock UI on the U60 Pro's 320×480 touch LCD with a
terminal-style status screen while a session runs, then hands the panel back.
It is a separate binary (`screen/`, installed as `/data/bin/zte-screen`) so a
display fault can never take the agent down; the agent only starts, stops and
watches it (`agent/src/screen.rs`).

## Pages

The bottom bar has seven pages and `:q`; swipe left/right also moves between
pages. Tap the clock to open `screen.conf` (the settings overlay).

| Page | Shows |
|---|---|
| HOME | 咕嘎, the companion in her scene (below), then uptime, load, CPU, memory, temperatures and power, WAN (RAT, band, RSRP, rates, IPv4, IPv6) and mihomo; the most severe alert takes the mihomo row |
| SYS | htop-style per-core CPU with frequency, memory with cache, tasks, load, governor, context switches and interrupts per second, 60 s CPU graph, temperatures, disk use, top processes |
| PWR | battery level, status, external power, charge limit, voltage, current, power, temperature, health, cycles, capacity versus design, time to full/empty, 2 min current graph, every thermal zone (hottest first) and throttling |
| CELL | RAT, band, bandwidth, operator, roaming, network mode, RSRP and SINR with ratings, RSRQ, RSSI, PCI, ARFCN, carrier aggregation, 3 min RSRP and SINR graphs |
| NET | WAN down/up rate (the modem's counters, which include IPA-offloaded traffic) and peaks, 60 s graph, session/today/month totals, WAN addresses, DNS, per-interface rates and Wi-Fi station counts, conntrack, TCP sockets, DHCP leases, mihomo mode, connections and totals |
| LAN | both Wi-Fi radios (SSID, channel, width, power, stations) and every DHCP client joined with its Wi-Fi link (band, signal, rate) |
| PROXY | mihomo: mode, live traffic and connections, every proxy group with its choice and delay, a group's nodes, the busiest connections, the subscriptions and the service ([below](#proxy-page)) |

Signal ratings use the dashboard's thresholds (`web-app/design.md` § Signal
ratings). Alerts: CPU ≥ 68 °C (warn) / 80 °C (critical), battery ≥ 45 °C,
battery ≤ 20 % / 10 % on battery, no or weak signal, no WAN IPv4, memory below
10 %, a stopped service, a disk ≥ 90 % full, active thermal throttling.

Everything the screen reads is read-only: procfs/sysfs, `get` ubus methods
(`zte_nwinfo_api nwinfo_get_netinfo`, `zwrt_data get_wwandst`, interface
status), `iw … station dump` / `info`, and GETs on mihomo's root-only
controller socket. The PROXY page's controls are the only writes, and only
when tapped.

## Controls

- **Touch:** tap tabs and the clock; swipe to change page. Two-finger touches are ignored.
- **Power key:** a short press turns the backlight off/on; holding it for 1.5 s ends the session (the stock UI then shows its own power menu if the key is still held).
- **`:q`** ends the session.
- **Idle:** after `idle_secs` (default 120 s) without a touch the session hands back to the stock UI (`idle_action: stock`, the default, so the firmware's own screen-off and sleep handling resumes) or turns the backlight off and stays (`sleep`: a short power press lights it again, and only `:q` or a long press hands back, however long it stays dark). A banner counts down the last 10 s. A dark session stops collecting stats; the panel keeps a UI, so the firmware's no-UI reboot (below) does not apply, and on this firmware the stock UI does not drive system sleep (`zte_topsw_sleep_faw` does).

## HOME companion

咕嘎 is DeepSeek drawn as a maid whale girl (`screen/src/pet/`). Her art (`screen/assets/guga.bin`) is cut from the owner's reference picture plus one face-only edit of it per expression made with Codex's image tool, and built by `screen/tools/guga_sprite.py` (the inputs are not vendored; the script's docstring has the recipe); small props (hearts, notes, the whale) are pixel art in `screen/assets/pet.txt`.

**Her day.** She goes through six scenes (`screen/src/pet/scene.rs`), each painted in three lights: golden (05–07 and 17–18), day (08–16) and night. The backgrounds (`screen/assets/scenes.bin`, raw RGB565, about 2.7 MB) were made with Codex's image tool in her art style — noon from a prompt, morning and night as edits of noon so they line up — and built by `screen/tools/scenes.py`. The routine: 卧室小屋 (23–06), 电车 (07–08 and 17), 算力机房 (09–11), 女仆咖啡厅 (12–13 and 21–22), 图书馆 (14–16), 天台 (18–20). Tapping the title bar takes her to the next scene; she stays until the routine moves on. Changes dissolve over 0.6 s, and she says a line on arrival. Each scene moves: dust in the sunbeams, steam from the espresso machine, a sparkle in the cake case, blinking racks, a live trace on the monitors and a breathing whale emblem in the datacenter, trees, utility poles and wires sweeping past the train windows with a rail jolt every 1.5 s, birds by day and stars, string lights, city windows, a plane and the odd shooting star by night, flickering lamps and the router's LED on her desk.

She only reacts to taps:

- **Head:** a pat (she bounces, hearts). **Body:** a poke; several in a row make her cross. **Elsewhere in the scene:** she is startled. **Title bar:** the next scene. While she dozes, the first tap only wakes her. **Her speech bubble:** shows the whole line, a second tap puts it away.
- On her own she wanders a little, hums, remarks on router events (charging or unplugged, battery at 20 % or less, CPU at 68 °C or more, a new Wi-Fi client, 50 Mb/s or more down, RSRP below −112 dBm, a new critical alert), greets you after 20 min away and dozes off at night (01:00–06:00 when the screen opens, or after a minute untouched from 23:00); a tap wakes her.
- There is nothing to feed or level up. The only state on disk is when she last saw you (`/data/local/tmp/zte-pet-state.json`).

**Her lines.** With `/data/local/tmp/zte-pet.json` (mode 600) she asks an OpenAI-compatible endpoint for each line; without it, or when a call fails, she uses built-in lines. `{"base_url": "https://…/v1", "api_key": "sk-…", "model": "…", "name": "咕嘎", "user": "主人", "per_hour": 60}`; `name`, `user` and `per_hour` are optional. The model is told it cannot operate the router and gets only a one-line read-only summary (time, battery, temperatures, cell, rates, traffic, client count, mihomo mode and connections, uptime, alerts — no addresses, identifiers or client names), which scene she is in, and what just happened. Its reply is only drawn: `{"say": "…", "mood": "…"}`, where the mood picks her face. Requests go through `curl -K /tmp/zte-screen/pet.curl` (mode 600, so the key is never on a command line), at most one in flight, 3 s apart, `per_hour` an hour, 25 s timeout; repeated taps reach the model at most every 10 s. `enable_thinking: false` keeps Qwen 3.5 quick. Failures go to `/tmp/zte-screen/panel.log` without the key.

Scanlines skip her scene, which is a picture rather than terminal text. **Cost.** A background is decoded once per scene and light and copied; HOME redraws at 15 fps while something moves, 12 fps on the train and 8 fps otherwise: about 9 % of one core while lit at rest (12 % on the train), and nothing while dark.

**Testing on the device:** `echo pat > /tmp/zte-screen/pet.request` (also `poke`, `spam`, `wake`, `doze`, `greet`, `chatter`, `travel`), then `touch /tmp/zte-screen/snapshot.request` for a frame. `echo "260 450" > /tmp/zte-screen/tap.request` taps the screen (one `x y` per line; `:q` is ignored).

## PROXY page

mihomo at a glance and the few controls the owner wanted (`screen/src/proxy/`).
While the page is open the panel reads the controller socket every second
(`/connections`; `/proxies` every 3 s or right after a change; `/configs` every
5 s).

- **Overview:** the mode (规则 / 全局 / 直连), down/up rate with a 60 s graph and the connection count, then every proxy group in config order (GLOBAL and hidden groups left out) with its choice, where that leads, and the leaf node's last delay. Tap a group for its nodes, the traffic line for connections. The bar: page up/down, 测速 (tests every node through GLOBAL), 连接, 订阅.
- **A group:** its members with delays, the current one marked. In a select group, tapping a node switches to it **at once** — the one change that asks for no confirmation. Auto groups (url-test, fallback, load-balance) only show what they chose. 测速 tests this group.
- **Connections:** the busiest 40 by current rate: host, group > node, rule.
- **订阅与服务:** mihomo's version, state, CPU and memory with 重启; each subscription with node count, last update, usage and expiry, or its error, with 更新; 全部更新.

Node switches and delay tests go to mihomo directly (`PUT /proxies/<group>`,
`GET /group/<group>/delay`); the profile's `store-selected` keeps a choice
across restarts. Everything else belongs to zte-agent, which owns mihomo's
config, and asks first in a confirmation box: changing the mode, restarting
mihomo, updating one or all subscriptions. The panel writes
`/tmp/zte-screen/agent.request` (the session directory is root-only), e.g.
`{"id": 7, "op": "mode", "mode": "global"}`; the agent's screen watcher picks it
up within 0.4 s while a session runs (a request older than 20 s is dropped,
so nothing runs late), runs the same handler as the dashboard's
API on a worker thread (one request at a time), logs it, and answers in
`agent.reply`. It accepts exactly `mode` (rule/global/direct), `restart`,
`update` (one subscription id, or all) and `subs` (the subscription list
without URLs); anything else is refused.

## Opening it

- **Dashboard:** System → Settings → Geek screen → *Show on device* (`POST /api/screen`).
- **Two-finger hold** (`gesture`, on by default): hold two fingers still on the *lit* stock UI for 2 s. The agent reads the touchscreen without grabbing it, so the stock UI also sees the touches; put the fingers where it has no button. One finger, a third finger, drift beyond 1/12 of the screen, a dark screen or dropped events never count; 5 s cooldown.
- **`auto_open`:** `off` (default); `boot` opens once per boot a few seconds after the stock UI is up (within 10 min of power-on); `wake` also opens whenever the stock UI lights a screen that has been dark for 1.5 s. After `:q` or a long power press it does not reopen until the screen has slept again. A refused start pauses automatic opens for 1 min, a failed one for 10 min, and there are at most 30 automatic opens per hour.

## Safety model

This is the one sanctioned exception to "never stop a daemon listed in
`zte_topsw_daemon.conf`" (docs/SAFETY.md). The rule exists because *disabling*
a barrier daemon wedges the boot. `zte-screen` never disables anything and never
runs at boot; it only stops the stock UI after boot, for the session, and always
starts it again. The sequence follows the community takeover verified on B31
([acloudpeng/U60Pro-OpenUI](https://github.com/acloudpeng/U60Pro-OpenUI) `screen/`,
[amenekowo/mu5250_tweaking](https://github.com/amenekowo/mu5250_tweaking) `docs/screen.md`,
[faying/luya-z-manager](https://github.com/faying/luya-z-manager) `touch-ui/docs/`):

1. **Preflight** (`zte-screen --preflight`): `/tmp/zte_boot_done` exists, `zwrt_topsw_daemon.sync get_sync_info` reports `sync success` and `register success`, the device is not in power-off charging (`mode_main_state`), the stock UI is running, the backlight is readable. Otherwise the session is refused and nothing is touched.
2. **Park:** `/etc/init.d/zte_topsw_devui stop` (procd stops respawning it; no `disable`, so no overlay whiteout). The stock UI ignores SIGTERM for seconds, so after 1.2 s the exact `/usr/bin/zte_topsw_devui` executable gets SIGKILL, as ZTE's own `startdui.sh` does. The backlight stays dark until a real frame is up.
3. **Panel child** (`zte-screen --panel`): DRM atomic KMS with two dumb RGB565 buffers; the first commit also sets the CRTC's `MODE_ID`/`ACTIVE` and the connector's `CRTC_ID`. **Never legacy `SETCRTC`** (QPIC timeout → reboot) and never `/sys/kernel/debug/qpic_display/image_dump` (reboot). The backlight is lit only after the first frame, then two settle commits (150/350 ms). Panel and touch axes are rotated 180°. Neither input device is grabbed.
4. **Heartbeat:** the child writes `H` every second; 10 s of silence and the supervisor kills it. An orphaned child (supervisor gone) restores the stock UI itself.
5. **Restore** on every exit path: `/etc/init.d/zte_topsw_devui start`, wait for the process, put the recorded brightness back. If both processes die, the agent's watcher sees the stock UI parked with no owner and runs `zte-screen --stop` within ~10 s.

Known hazards from the community work, all avoided by design: two processes
committing to `/dev/dri/card0` (eventually reboots — one owner, the lock in
`/tmp/zte-screen/lock`); the screen left without any UI for minutes (the
firmware reboots, reason 1185 — every stop path starts the other UI in the same
step); a UI that crashes at boot under procd (escalated to a reboot loop — the
screen never starts at boot by itself, `auto_open: boot` waits for the barrier
and the stock UI).

State lives in `/tmp/zte-screen/` (`state`, `owner`, `child`, `ui`,
`brightness`, `screen.log`, `panel.log`, and the PROXY page's
`agent.request`/`agent.reply`); settings in
`/data/local/tmp/zte-screen.json`. `touch /tmp/zte-screen/snapshot.request`
makes a running panel write its current frame to `/tmp/zte-screen/frame.png`.

## Build, preview, deploy

```sh
cargo test -p zte-screen
cargo run -p zte-screen -- --preview /tmp/screen-preview   # every page as PNG, synthetic data
./deploy-screen.sh --dry-run && ./deploy-screen.sh         # transactional, like deploy.sh
```

On the device: `zte-screen --supervise [SECONDS]` (a bounded test session),
`--stop` (also repairs an orphaned session), `--status`, `--preflight`.

Chinese text in the speech bubble uses a GB2312 subset of GNU Unifont 16.0.04 (`screen/fonts/cjk16.bin`, built by `screen/tools/cjk_subset.py`; SIL OFL 1.1, `screen/fonts/LICENSE.unifont`).
The bitmap font is Spleen 2.2.0 (BSD-2-Clause, `screen/fonts/LICENSE.spleen`),
converted by `screen/tools/bdf2rs.py`.
