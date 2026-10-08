# Agent — `zte-agent`

Rust HTTP backend that runs on the modem (`agent/`), talking to ubus, AT
ports, sysfs/procfs and device services. `agent/src/server.rs` is the
canonical routing table; this document summarizes it.

- Binds `192.168.0.1:9090` (override `ZTE_AGENT_BIND`, `ZTE_AGENT_THREADS`).
  Because it does not listen on device loopback, ADB setup verification runs
  an on-device curl against this LAN address; forwarding to `tcp:9090` and
  changing the HTTP Host header would not reach the listener.
- Auth: `POST /api/auth/login` (password from `ZTE_AGENT_PASSWORD`, or an
  optional 6-digit mobile PIN); bearer tokens with a sliding 1 h expiry so a
  dashboard left open stays logged in, rate-limited login, LAN-only CORS
- JSON envelope: `{ "ok": true, "data": … }` / `{ "ok": false, "error": … }`
- Destructive actions (`/api/device/reboot|shutdown`) require the
  `X-Confirm: true` header

## Endpoint reference

Every route below has a dashboard consumer, and every call the dashboard makes
is a route below — `scripts/check-api-contract.py` enforces both directions
(plus that the mock agent stays in step). 90 paths / 111 method+path pairs.

| Family | Endpoints |
|---|---|
| Auth | `POST /api/auth/login` — bearer token, sliding 1 h expiry |
| Batch | `GET /api/dashboard` — device, battery, cpu, memory, speed, data usage, signal, wan, wan6, thermal in one request. The app's heartbeat: Home, Signal and Modem/Data all read it instead of polling their own endpoints |
| Status | `GET /api/device`, `/api/cpu`, `/api/memory`, `/api/system/top` |
| Network | `GET /api/network/clients` (with `name`, the user-set name); `PUT /api/network/clients/name` (stock `router_modify_lan_hostname`); `POST /api/network/clients/kick` (Wi-Fi only, `zwrt_wlan kick_macs`); `GET+PUT /api/network/blocklist` (the Wi-Fi MAC filter in deny mode on every AP, applied live by hostapd; the firmware's `maclist` mirror is checked after each change because `/lib/wifi/zteqcawifi.sh` builds the deny file from it at Wi-Fi start) |
| Device | `GET /api/device/battery-info`, `/api/device/thermal/all`, `/api/device/battery/detail`, `/api/device/charger`; `POST /api/device/reboot`, `/api/device/shutdown`; `GET+PUT /api/device/sleep` (idle minutes before the device sleeps, `-1` = never; stock `set_ufi_sleep`); `GET+PUT /api/device/reboot-schedule` (weekly or every N days, time and random delay window; stock `set_device_info`, read from uci `zwrt_zte_mc.reboot_schedule`) |
| System | `POST /api/system/restart-agent`, `/api/system/kill-bloat` |
| Wi-Fi | `GET /api/wifi/status`, `PUT /api/wifi/settings`. Guest network: `guest_ssid`, `guest_key`, `guest_encryption` and `guest_hidden` go to both guest APs through uci like the main network; `guest_disabled_2g/5g` and `guest_active_time` (minutes: 0, 120, 240, 480, 720) go through the stock `zwrt_wlan set`, which runs the firmware's guest timer (`guest_left_secs` in the status). The stock call rejects a plaintext key, hence the split. An open guest network without a time limit is refused, as in the stock UI |
| Modem | `PUT /api/data-usage/reset-day`, `PUT /api/modem/network-mode`; `GET+PUT /api/modem/data` (mobile data connect/disconnect; link state from `connect_status`, the PUT waits up to 8 s for it to settle); `GET+PUT /api/data-usage/limit` (monthly data limit in bytes plus alert percentage; a time-based limit set in the stock UI is reported read-only) |
| Cell/band lock | `POST /api/cell/lock/nr`, `/api/cell/lock/lte`, `/api/cell/lock/reset`, `/api/cell/band/nr`, `/api/cell/band/lte`, `/api/cell/band/reset` |
| Router | `GET+PUT /api/router/dns`, `/api/router/lan`, `/api/router/apn/mode`; `GET+POST+PUT /api/router/apn/profiles` (PUT edits a profile in place with the stock `modify_manu_apn`, carrying over `isEnable`/`cid`/`roamingPdpType` from the current entry); `POST /api/router/apn/profiles/delete`, `/api/router/apn/profiles/activate` |
| Network services | `GET+PUT /api/router/watchdog` (stock connection watchdog; enabling waits 10 s because the firmware turns it back off when it cannot ping the address → 409); `GET+PUT /api/router/firewall` (UPnP, DMZ host, remote management, WAN ping); `GET+POST+PUT /api/router/port-forwards`, `POST /api/router/port-forwards/delete` (port forwarding = a range to the same ports, port mapping = one port to another; max 20 each, overlapping external ports rejected; rules read from uci `firewall` redirects); `GET+POST+PUT /api/router/dhcp-bindings`, `POST /api/router/dhcp-bindings/delete` (MAC-IP binding, max 10, applied by the firmware after a reboot); `GET /api/system/time` (clock status, read only) |
| Cellular | `GET /api/cell/operators`, `POST /api/cell/operators/scan`, `/api/cell/operators/select` (`{mccmnc, rat}`), `/api/cell/operators/auto` — manual carrier selection through the stock `nwinfo_manual_scan` / `nwinfo_manual_register` flow (the dashboard polls GET); back to automatic re-applies the network mode and waits for `net_select_mode` |
| SMS forwarding | `GET+PUT /api/sms/forward`, `POST /api/sms/forward/test` — a background thread polls the newest messages every 20 s and pushes received ones above `last_id` to Bark, Server酱, a WeCom group bot, Telegram (optionally through the local mihomo proxy) or a JSON webhook. History is not sent; the key/URL is kept 0600 in `/data/local/tmp/sms_forward.json` and never returned |
| Client traffic | `GET /api/network/clients/traffic`, `POST /api/network/clients/traffic/reset` — per-device internet bytes from conntrack accounting (IPA offload syncs its counts back), sampled every 10 s by client MAC, saved every 5 min to `/data/local/tmp/client_traffic.json` |
| Backup | `GET /api/system/backup` (one JSON document: proxy state without the controller secret, SMS forwarding, sleep, reboot schedule, watchdog, UPnP/DMZ/remote access, port rules, fixed addresses, monthly limit, block list, device names; holds subscription links and push keys), `POST /api/system/restore` (X-Confirm; optional `only: [sections]`; each section goes through the normal handlers and reports ok/partial/failed; rules, bindings and block-list entries are added when missing, never deleted; profile configs are downloaded again) |
| SMS | `POST /api/sms/list`, `/api/sms/send`, `/api/sms/delete`, `/api/sms/read` (delete falls back to direct SQLite for SIM-stored rows the firmware refuses) |
| SIM | `GET /api/sim/info`, `/api/sim/imei` |
| USB | `GET /api/usb/status`, `PUT /api/usb/mode`, `/api/usb/default`, `/api/usb/powerbank` |
| Power | `GET+PUT /api/device/charge-control` — manual stop/resume + limit enforcer with hysteresis, event-driven off `BSP_CHARGER_EVENT` |
| Extras | TTL clamping (`GET /api/ttl/status`, `PUT /api/ttl/set`, `DELETE /api/ttl/clear`), AT console (`POST /api/at/send`, `GET /api/at/port`), signal/connection CSV loggers (`/api/logger/*`) |
| Proxy (mihomo) | `GET /api/proxy/status`; `PUT /api/proxy/settings` (mode, preset, tun, cn_bypass, mixed_port); `POST /api/proxy/service` (start/stop/restart); `GET`/`POST`/`PUT /api/proxy/subscriptions`, `POST /api/proxy/subscriptions/delete` (X-Confirm), `POST /api/proxy/subscriptions/update`; `GET`/`PUT /api/proxy/groups`; `POST /api/proxy/delay`. Plus unauthenticated `GET /proxy.pac` for LAN devices |

## Architecture notes

- **Transport**: `tiny_http` thread pool; no async runtime (small binary,
  small footprint). The listener is supervised — tiny_http's accept thread
  exits permanently on its first `accept()` error, so `server::start` watches
  for that, drains the workers and rebuilds rather than sitting alive serving
  nothing.
- **Dependencies**: `serde`, `serde_json`, `tiny_http`, `sha2`, `libc`. No TLS
  stack and no HTTP client — removing the DoH proxy, SMS forwarder and speed
  test dropped `ureq`, and with it rustls/ring/ICU.
- **Subprocess cost**: every `ubus`/`uci` read is a fork+exec, which dominates
  the agent's CPU (about 4–5 ms per `ubus call` on-device). `cache.rs` gives
  each dashboard source its own TTL (signal 1 s, WAN throughput 1 s, thermal 10 s, wan/wan6 30 s, data usage 30 s, cycle dates 300 s), so
  the client's poll rate is decoupled from the refresh rate and concurrent
  clients collapse onto one refresh. `wifi_status` dumps whole configs with
  `ubus::uci_show` instead of issuing one `uci get` per key.
- **Event bus**: one `ubus listen` process dispatches to subscribers over
  bounded channels (`BSP_CHARGER_EVENT` → charge enforcer).
- **State files** (all under `/data/local/tmp/`): `charge_limit.json`,
  `usb_config.json`, signal/connection CSV logs.
- **Boot behavior**: `main.rs` runs a one-shot migration that undoes the
  removed DoH proxy's dnsmasq rewiring (otherwise a device that had DoH
  enabled would come back up forwarding DNS to a dead port), applies
  `start_ttl.sh` if present, and re-applies persisted NCM only if explicitly
  enabled — see [SAFETY.md](SAFETY.md) §2 for why the latter two are acceptable.
- **Logging**: the agent redirects its own stdout/stderr (`diag_log.rs`) to
  `/data/local/tmp/zte-agent.log`. Lines carry the device's local time and uptime.
  Each file is capped at 256 KiB, one rotated `.1` copy is kept, and identical
  consecutive lines are collapsed into a count. Syslog isn't usable here: the
  stock busybox `syslogd` runs with `-l 1` (emergency only) and has no `logread`
  backend. The startup script sends the agent's output to `/dev/null`, and the
  agent redirects its own stdout/stderr at startup. Timestamps in this log and
  in the CSV logs are device-local time without a `Z`. The firmware keeps local
  time in the system clock with `TZ=UTC`, so epoch values from the device
  differ from true UTC by the local offset.

## Safety constraints built into the agent

- **AT console is allowlisted** (`server.rs`): read-only commands only;
  `AT+CFUN`, `AT^…`, `AT+CMGD`, `AT$QCRMCALL`, `AT+CLCK`, `AT+CGDCONT=`,
  `AT+CGACT=` are blocked.
- **kill-bloat only kills daemons that are safe to kill** — never the
  `zte_topsw_daemon.conf` sync-barrier set (see SAFETY.md).
- **Destructive endpoints require `X-Confirm: true`** (`/api/device/reboot`,
  `/api/device/shutdown`).
- **Login is rate-limited**: 5 failures per client IP arms a 30 s lockout.
- **LAN-only bind + LAN-origin CORS** by default.
- ubus inputs passed through from HTTP are size/depth-validated
  (`validate.rs`) before forwarding.

## Proxy (mihomo)

`agent/src/mihomo/` manages a [mihomo](https://github.com/MetaCubeX/mihomo)
core installed in `/data/mihomo` by `scripts/deploy-mihomo.sh` (pinned
release, GitHub SHA-256 verified on both ends).

- **State**: `/data/mihomo/manager.json` (0600) holds settings and
  subscriptions. The API returns subscription links masked
  (`https://host/…`); full links never leave the router.
- **Config**: rendered as JSON from typed state (`config.rs`), validated with
  `mihomo -t`, atomically renamed into `config.yaml`, then hot-reloaded via the
  controller. A failed reload restores the previous file and state.
  Subscriptions are fetched `DIRECT` so they never depend on their own nodes.
- **Clock**: the firmware keeps local time in the system clock labelled as
  UTC (8 h ahead of real UTC in China), and its time daemons rewrite it, so the
  agent never touches it. Every rendered config enables mihomo's own NTP with
  `write-to-system: false`; without it Shadowsocks 2022 (and REALITY servers
  that enforce a time window) reject every connection right after TCP connects.
- **Upgrades**: on start the agent re-renders the config and, if it differs
  from the installed file, validates and hot-reloads it, so fixes in a new
  agent reach an adopted mihomo without a restart.
- **Controller and panel**: mihomo listens on `<LAN IP>:9097` and serves the
  metacubexd panel at `/ui/` (installed by `deploy-mihomo.sh` from a pinned,
  digest-verified release; its `config.js` points it at the page's own origin).
  The controller's `secret` is the subscription config's own (profile mode),
  otherwise empty, i.e. **no password**: any LAN device can open the panel and
  control the proxy. The agent uses the controller's unix socket
  `/data/mihomo/mihomo.sock` instead, where mihomo applies no secret, so the
  agent works whatever secret the config sets. `GET /api/proxy/status` reports
  `panel {installed, url, secret}`. An agent upgrading from the loopback-only
  controller restarts mihomo once (the old process has no socket to reload
  through). A hot reload does not replace the controller's secret, so a config
  whose secret changed (e.g. after a subscription update) restarts mihomo, and
  at start the agent checks that the controller accepts the configured secret
  and restarts it if not.
- **Listener**: the mixed HTTP/SOCKS port binds the LAN address only.
- **Process**: started in its own session so agent restarts do not stop it;
  a new agent re-adopts it from `/var/run/mihomo.pid`. The watchdog (5 s tick)
  restarts it with exponential backoff (max 5 min), and turns TUN off after 3
  crashes in 10 minutes.
- **TUN** (`settings.tun`): device `mihomo`, `include-interface: br-lan` only,
  so the router's own traffic never enters it; DNS is left to dnsmasq and
  domains come from the TLS/HTTP/QUIC sniffer. ZTE's fw3 rejects LAN→tun
  forwarding and tun INPUT, so while TUN runs the agent inserts tagged
  (`mihomo-tun`) iptables/ip6tables accepts at the top of FORWARD and INPUT,
  re-asserts them every 15 s (fw3/QCMAP reloads flush them), and removes them on
  stop. A force-killed mihomo's ip rules (prefs 9000–9099, table 2022) are
  cleaned up. Nothing is written to the firmware: with TUN enabled the agent
  re-applies the rules at boot; turning TUN off restores the stock route.
- **Mainland bypass** (`settings.cn_bypass`, default on): TUN costs about one
  A55 core per 100 Mbit/s (measured 2026-10-06: ~90 Mbit/s domestic at 80–90%
  of a core). With the bypass, LAN packets to mainland IPv4 ranges (ipset
  `mihomo-cn`, loaded from `/data/mihomo/cn.list`, which `deploy-mihomo.sh`
  installs from a pinned MetaCubeX commit) get mark `0x10000000` in mangle
  PREROUTING, and ip rule 8999 sends them through the main table ahead of
  sing-tun's rules, so they keep the stock path and IPA offload (measured
  138–195 Mbit/s at ~0% mihomo CPU). Runtime only, re-asserted with the
  accepts, removed with them.
- **Forwarding watchdog**: every 60 s a mainland 204 endpoint is fetched
  through the mixed port (and directly when that fails, to rule out a WAN
  outage). Three failures through mihomo restart it; if it still does not
  forward, TUN is turned off so LAN clients regain the normal route.
- **Device test**: `mihomo::device_tests::device_e2e` (ignored) exercises start,
  TUN, hot reload, kill-and-restart and firewall re-assertion on real hardware.

## USB modes

See [reference/usb-modes.md](reference/usb-modes.md) for the live-device
findings: only ECM/RNDIS are exposed by the stock switch; NCM exists in
configfs and is agent-managed (experimental, gated behind
`confirm_experimental`), and the ubus `mode` field is not a reliable
detector of the active composition.

CN firmware B31 removed the stock switch itself: `zwrt_bsp.usb` exposes only
`list`. The agent checks for `set` once (`ubus -v list`), reports
`mode_switch: false`, marks RNDIS unsupported and refuses ECM/RNDIS requests
with 409 instead of failing with "Method not found". The configfs NCM path
does not depend on it.

## Building

```sh
cargo build --release --target aarch64-unknown-linux-musl -p zte-agent
```

Cross-linker config lives in `.cargo/config.toml`
(`aarch64-linux-musl-gcc`). `cargo test` runs the unit tests (auth lockout
and token expiry, dashboard payload shapes, TTL cache, UCI value unquoting,
USB boot guards, WiFi sanitizers).

`python3 scripts/check-api-contract.py` asserts the agent route table, the
dashboard's calls and the mock agent's fixtures all agree — run it after
touching any of the three.

## Recovery, freshness and bounded logging

The default listener reads the configured LAN IPv4 address at startup. A LAN
change returns HTTP 202 with a reconnect address and a confirmation token. The
dashboard confirms connectivity within 120 seconds. Until confirmation, a
private recovery record lets the agent restore the previous settings after a
timeout or restart. A fixed `ZTE_AGENT_BIND` override blocks IP changes. The
confirmation endpoint accepts only the token scoped to that pending change;
it does not require forwarding the general session token to the new address.

Dashboard `sources` metadata reports the last successful sample time, age,
refresh interval, staleness and collection error for signal, WAN, IPv6 WAN,
thermal and data-usage sources. Failed refreshes retain the last successful
reading and mark it stale. The UI shows source failures and charge-policy
errors, and warns when its entire dashboard refresh fails.

`data_usage` (in `/api/dashboard` and the `PUT /api/data-usage/reset-day`
reply) reports counters as numbers, or `null` when the firmware omits or
garbles one. `reset_day` (1–31) and `reset_enabled` (0/1) come from
`zwrt_data get_wwandst_clearday`, falling back to the persisted
`zwrt_data_commit.wwancid1dst` UCI values; each is `null` when neither source
supplies a valid value. `clear_date_record` (`YYYY/MM/DD`) and
`next_clear_date` (`YYYYMMDD`) are passed through as the firmware stores them.
`since_power_on` holds the firmware's `real_*` counters; on HK B04 these reset
with the data connection, not only at power-on, so the dashboard labels them
connection counters. Saving a reset day also enables the automatic reset.

Signal and connection loggers share the dashboard's radio source (one-second
minimum refresh interval). CSV files have an 8 MiB cap per logger, buffered
writes, a 30-second maximum flush interval and an error field in logger status.
Downloads stream a fixed-length snapshot as `text/csv`, retaining the same
endpoint paths. The updated dashboard also understands older JSON-wrapped CSV
responses. A logger stops on write/flush failure rather than counting failed
writes as successful samples.

Charge policy reconciles periodically even when charger events are lost. USB
switches are serialised, abort when boot readiness cannot be established, and
attempt to restore the previous composition and bridge after a failed switch.
Physical USB and charging behaviour must still be checked on each supported
firmware; local failure tests do not establish hardware compatibility.
