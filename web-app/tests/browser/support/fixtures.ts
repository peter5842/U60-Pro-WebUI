/**
 * Raw agent payload builders for the browser regression harness.
 *
 * Every builder returns the JSON that the agent puts in the `data` field of its
 * envelope, i.e. the shape BEFORE the dashboard mappers in `src/data/api.ts`
 * run. Each takes a partial override that is deep-merged over the defaults
 * (plain objects merge key by key; arrays, scalars and null replace).
 *
 * Provenance: every value is synthetic. Shapes marked "sanitised shape based on
 * HK B04" follow what the device and agent/src/*.rs produce, with all
 * identifiers (IMEI, ICCID, IMSI, MSISDN, MAC, SMS content, SSIDs, passwords,
 * public addresses) replaced. Nothing here was copied from a live unit.
 */

export type Json = Record<string, unknown>
export type NetworkKind = 'SA' | 'NSA' | 'LTE' | 'disconnected'

function isPlain(v: unknown): v is Json {
  return typeof v === 'object' && v !== null && !Array.isArray(v)
}

/** Deep-merge `over` into a copy of `base`. */
export function merge<T extends Json>(base: T, over?: Json): T {
  const out: Json = { ...base }
  for (const [k, v] of Object.entries(over ?? {})) {
    out[k] = isPlain(v) && isPlain(out[k]) ? merge(out[k] as Json, v) : v
  }
  return out as T
}

// ── /api/dashboard ───────────────────────────────────────────────────────────

/**
 * Raw `zte_nwinfo_api nwinfo_get_netinfo` payload per network state.
 * Provenance: SA = sanitised shape based on HK B04 (values observed on the
 * device, cell ids/carrier replaced); NSA/LTE/disconnected = synthetic, in the
 * same key layout.
 */
export function netinfo(kind: NetworkKind = 'SA', over?: Json): Json {
  const bandLocks = '1,2,3,5,7,8,18,20,26,28,29,38,40,41,48,66,71,75,77,78,79'
  const common: Json = {
    network_provider_fullname: 'Synthetic Carrier',
    network_provider: 'SynCarrier',
    lock_lte_cell: '',
    lock_nr_cell: '',
    lte_band_lock: '0x87e29a0e00df',
    nr5g_sa_band_lock: bandLocks,
    nr5g_nsa_band_lock: bandLocks,
  }
  let base: Json
  switch (kind) {
    case 'SA':
      base = {
        ...common,
        network_type: 'SA',
        net_select: 'Only_5G',
        signalbar: '5',
        cell_id: 0,
        nr5g_cell_id: 5_000_000_123,
        nr5g_pci: 745,
        nr5g_action_channel: 643392,
        nr5g_action_band: 'n78',
        nr5g_bandwidth: '100',
        nr5g_rsrp: -53,
        nr5g_rsrq: -11,
        nr5g_snr: '31.0',
        lte_rsrp: -48,
        lte_rsrq: -7,
        lte_snr: '21.0',
        lte_pci: 0,
        wan_active_channel: 0,
        wan_active_band: '',
        lteca: '',
        nrca: '',
      }
      break
    case 'NSA':
      base = {
        ...common,
        network_type: 'ENDC',
        net_select: 'WL_AND_5G',
        signalbar: '4',
        cell_id: 134_479_973,
        lte_pci: 312,
        wan_active_channel: 3650,
        wan_active_band: 'B8',
        lte_rsrp: '-71',
        lte_rsrq: '-10.8',
        lte_snr: '19.5',
        lte_rssi: '-62',
        lteca: '312,8,1,3650,20,1;314,1,1,300,15,1',
        ltecasig: '-79,-12.1,14.0,-71,1,2',
        nr5g_rsrp: '-77',
        nr5g_action_band: 'n78',
        nr5g_action_channel: 630912,
        nr5g_pci: 801,
        nr5g_bandwidth: '100',
        nr5g_snr: '23.5',
        nr5g_rsrq: '-9.2',
        nr5g_rssi: '-58',
        nr5g_cell_id: 268_566_611,
        // Second entry is a configured but unmeasured SCC (reporting-floor values).
        nrca: '1,803,2,40,472000,40,0,-88,-11.4,12.5,-69;0,56,1,78,643392,60,1,-140.0,-43.0,-23.0,-120.0;',
      }
      break
    case 'LTE':
      base = {
        ...common,
        network_type: 'LTE',
        net_select: 'Only_LTE',
        signalbar: '3',
        cell_id: 134_479_973,
        lte_pci: 312,
        wan_active_channel: 3650,
        wan_active_band: 'B8',
        lte_rsrp: '-93',
        lte_rsrq: '-12.5',
        lte_snr: '9.0',
        lte_rssi: '-65',
        lteca: '312,8,1,3650,20,1',
        nr5g_rsrp: '',
        nr5g_action_band: '',
        nrca: '',
      }
      break
    case 'disconnected':
      base = {
        ...common,
        network_type: 'No Service',
        net_select: 'WL_AND_5G',
        signalbar: '0',
        cell_id: 0,
        network_provider_fullname: '',
        network_provider: '',
        lte_band_lock: '0',
        nr5g_sa_band_lock: '',
        nr5g_nsa_band_lock: '',
      }
      break
  }
  return merge(base, over)
}

/** Freshness metadata as produced by agent/src/cache.rs (`Observed::read`). */
export function freshness(over?: Partial<{ sampled_at_ms: number | null; age_ms: number | null; ttl_ms: number; stale: boolean; error: string | null }>): Json {
  return { sampled_at_ms: 1_790_000_000_000, age_ms: 300, ttl_ms: 1000, stale: false, error: null, ...over }
}

export function sources(over?: Record<string, Json>): Json {
  const base: Record<string, Json> = {
    signal: freshness({ ttl_ms: 1000 }),
    wan: freshness({ ttl_ms: 30_000 }),
    wan6: freshness({ ttl_ms: 30_000 }),
    thermal: freshness({ ttl_ms: 10_000 }),
    data_usage: freshness({ ttl_ms: 30_000 }),
    speed: freshness({ ttl_ms: 1000 }),
  }
  return merge(base, over)
}

/**
 * Data-usage payload (agent/src/handlers.rs `read_data_usage_live`).
 * Reset config is the sanitised shape from the device: `reset_enabled` is a
 * number, and the two cycle dates use DIFFERENT formats ('YYYY/MM/DD' for
 * `clear_date_record`, 'YYYYMMDD' for `next_clear_date`).
 */
export function dataUsage(over?: Json): Json {
  return merge(
    {
      day: { rx_bytes: 2_350_000_000, tx_bytes: 118_000_000, time_secs: 32_400, rx_packets: 1_900_000, tx_packets: 800_000 },
      month: { rx_bytes: 64_800_000_000, tx_bytes: 3_900_000_000, time_secs: 640_000, rx_packets: 50_000_000, tx_packets: 20_000_000 },
      cycle: { rx_bytes: 64_800_000_000, tx_bytes: 3_900_000_000, time_secs: 640_000, rx_packets: 50_000_000, tx_packets: 20_000_000 },
      since_power_on: { rx_bytes: 18_400_000_000, tx_bytes: 1_260_000_000, time_secs: 384_200, rx_packets: 14_000_000, tx_packets: 6_000_000 },
      total: { rx_bytes: 402_000_000_000, tx_bytes: 21_700_000_000, time_secs: 4_120_000, rx_packets: 300_000_000, tx_packets: 120_000_000 },
      reset_enabled: 1,
      reset_day: 16,
      clear_date_record: '2026/09/16',
      next_clear_date: '20261016',
    },
    over,
  )
}

export interface DashboardOptions {
  /** Which radio state to model. Default 'SA'. */
  network?: NetworkKind
  /** Deep-merged over the netinfo for the chosen state. */
  signal?: Json | null
  battery?: Json | null
  cpu?: Json | null
  memory?: Json | null
  speed?: Json | null
  device?: Json | null
  data_usage?: Json | null
  wan?: Json | null
  wan6?: Json | null
  thermal?: Json | null
  sources?: Record<string, Json>
  charge_control_error?: string | null
}

/**
 * The /api/dashboard batch (agent/src/handlers.rs `dashboard`): the app's
 * heartbeat. Pass `null` for a section to model an unavailable source.
 * Provenance: sanitised shape based on HK B04.
 */
export function dashboard(opts: DashboardOptions = {}): Json {
  const network = opts.network ?? 'SA'
  const up = network !== 'disconnected'
  const section = <T extends Json>(base: T, over: Json | null | undefined): T | null =>
    over === null ? null : merge(base, over)
  return {
    device: section(
      {
        hostname: 'U60-Pro',
        uptime_secs: 384_200,
        load_avg: [0.42, 0.35, 0.31],
        kernel: 'Linux version 5.15.170-perf (builder@example) (aarch64-openwrt-linux-musl-gcc 12.3.0) #1 SMP PREEMPT',
        firmware: 'XCBZ_HK_MU5250V1.0.0B04',
        hardware: 'MU5250_HW1.0',
      },
      opts.device,
    ),
    battery: section(
      {
        capacity: 78,
        status: 'Charging',
        voltage_uv: 4_210_000,
        temperature: 330, // tenths of a degree
        current_ua: 1_450_000,
        external_power: true,
      },
      opts.battery,
    ),
    cpu: section({ overall: 23.4, cores: [31, 22, 19, 20] }, opts.cpu),
    memory: section({ total_kb: 1_638_000, used_kb: 612_000, free_kb: 1_026_000, usage_pct: 37.4 }, opts.memory),
    // Mirrors agent/src/system.rs::SpeedSnapshot. Rates are bytes/sec.
    speed: section(
      {
        rx_bytes: 18_400_000_000,
        tx_bytes: 1_260_000_000,
        rx_speed: up ? 76_250_000 : 0,
        tx_speed: up ? 4_750_000 : 0,
        max_rx_speed: 89_000_000,
        max_tx_speed: 5_750_000,
        elapsed_ms: 3000,
      },
      opts.speed,
    ),
    data_usage: opts.data_usage === null ? null : dataUsage(opts.data_usage ?? undefined),
    signal: opts.signal === null ? null : netinfo(network, opts.signal ?? undefined),
    wan: section(
      up
        ? {
            up: true,
            'ipv4-address': [{ address: '10.0.64.14' }],
            route: [{ nexthop: '10.0.64.1' }],
            'dns-server': ['10.0.64.1'],
            proto: 'qmi',
          }
        : { up: false },
      opts.wan,
    ),
    wan6: section(
      up
        ? {
            up: true,
            'ipv6-address': [{ address: '2001:db8:0:4a00::1c', mask: 64 }],
            'ipv6-prefix': [{ address: '2001:db8:0:4a00', mask: 64 }],
            'dns-server': ['2001:db8::1'],
          }
        : { up: false },
      opts.wan6,
    ),
    thermal: section({ cpuss_temp: 61 }, opts.thermal),
    sources: sources(opts.sources),
    charge_control_error: opts.charge_control_error ?? null,
  }
}

// ── Device ───────────────────────────────────────────────────────────────────

export const device = (over?: Json): Json => merge(dashboard().device as Json, over)
export const cpu = (over?: Json): Json => merge(dashboard().cpu as Json, over)
export const memory = (over?: Json): Json => merge(dashboard().memory as Json, over)

/** sanitised shape based on HK B04 (agent/src/device_ext.rs). */
export function batteryDetail(over?: Json): Json {
  return merge(
    {
      available: true,
      capacity: 78,
      status: 'Charging',
      voltage_mv: 4210,
      voltage_max_mv: 4500,
      voltage_ocv_mv: 4190,
      current_ma: 1450,
      power_mw: 6105,
      temperature_c: 33.0,
      charge_type: 'Fast',
      health: 'Good',
      cycle_count: 214,
      charge_counter_mah: 7800,
      charge_full_mah: 9410,
      charge_full_design_mah: 10000,
      time_to_full_secs: 3300,
      time_to_empty_secs: -1,
    },
    over,
  )
}

export function batteryInfo(over?: Json): Json {
  return merge(
    { available: true, online: true, low_power: false, using_hw_fg_chip: true, time_to_full_mins: 55, time_to_empty_mins: 0 },
    over,
  )
}

export function thermalAll(over?: Json): Json {
  return merge(
    {
      available: true,
      cpu_0: 61.2, cpu_1: 60.8, cpu_2: 59.7, cpu_3: 60.1,
      modem: 55.0, modem_ss0: 52.0, modem_ss1: 51.0, modem_ss2: 50.0,
      battery: 33.0, usb: 38.0, eth_phy: 44.0, pmic: 49.0, xo_therm: 35.0, pa: 47.0, sdr: 45.0,
    },
    over,
  )
}

/**
 * `zwrt_bsp.charger` passthrough. Firmware quirk kept on purpose:
 * direct_power_supply_mode 'enable' STOPS charging.
 */
export function charger(over?: Json): Json {
  return merge({ otg_powerbank_state: 0, direct_power_supply_mode: 'disable' }, over)
}

export function chargeControl(over?: Json): Json {
  return merge(
    {
      available: true,
      battery_available: true,
      charger_available: true,
      charging_stopped: false,
      battery_status: 'Charging',
      capacity: 78,
      charge_limit_enabled: false,
      charge_limit: 90,
      hysteresis: 5,
      manual_override: false,
      last_error: null,
    },
    over,
  )
}

// ── Modem / SIM / SMS / APN ──────────────────────────────────────────────────

export function modemCapabilities(over?: Json): Json {
  return merge(
    {
      network_modes: [
        { value: 'WL_AND_5G', label: '5G / 4G / 3G' },
        { value: 'LTE_AND_5G', label: '5G NSA' },
        { value: 'Only_5G', label: '5G SA' },
        { value: 'WCDMA_AND_LTE', label: '4G / 3G' },
        { value: 'Only_LTE', label: '4G only' },
        { value: 'Only_WCDMA', label: '3G only' },
      ],
      lte_bands: [1, 2, 3, 4, 5, 7, 8, 18, 19, 20, 26, 28, 29, 32, 34, 38, 39, 40, 41, 42, 43, 48, 66, 71],
      nr_sa_bands: [1, 2, 3, 5, 7, 8, 18, 20, 26, 28, 29, 38, 40, 41, 48, 66, 71, 75, 77, 78, 79],
      nr_nsa_band_lock_supported: false,
    },
    over,
  )
}

/**
 * Identifiers are placeholders on purpose; never put real ones here. They are
 * built at runtime so scripts/check-device-secrets.py (which rejects any
 * identity-shaped literal) stays meaningful.
 */
const placeholderDigits = (prefix: string, length: number) => prefix + '0'.repeat(length - prefix.length)

export function simInfo(over?: Json): Json {
  return merge(
    {
      sim_iccid: placeholderDigits('89', 19),
      sim_imsi: placeholderDigits('00101', 15),
      sim_states: 'SIM_READY', mdm_mcc: '001', mdm_mnc: '01',
    },
    over,
  )
}
export const simImei = (over?: Json): Json => merge({ imei: placeholderDigits('', 15) }, over)

export function smsCapabilities(over?: Json): Json {
  return merge({ available: true, ready: true, object: 'synthetic_wms', storage: 'native' }, over)
}

/**
 * Response of POST /api/sms/list (the app posts `{page:0, per_page:500}`).
 * `tag`: 0 = unread inbox, 1 = read inbox, 2 = sent (firmware convention).
 */
export function smsList(over?: { messages?: Json[] }): Json {
  return {
    messages: over?.messages ?? [
      { id: 3003, number: '+10000000001', content: 'Synthetic message three', date: '2026-08-08 16:42:11', tag: 0, mem_store: 1 },
      { id: 3002, number: 'SynCarrier', content: 'Synthetic message two', date: '2026-08-07 09:15:02', tag: 1, mem_store: 1 },
      { id: 3001, number: '+10000000002', content: 'Synthetic sent message', date: '2026-08-06 18:40:12', tag: 2, mem_store: 1 },
    ],
  }
}

export const mobileData = (over?: Json): Json =>
  merge(
    {
      connected: true,
      connect_status: 'ipv4_ipv6_connected',
      auto_connect: true,
      roaming_allowed: true,
      ipv4: '10.0.0.2',
      ipv6: '2001:db8::2',
    },
    over,
  )

export const dataLimit = (over?: Json): Json =>
  merge({ enabled: false, kind: 'data', limit_bytes: 107374182400, alert_percent: 80 }, over)

export const sleep = (over?: Json): Json => merge({ minutes: -1, options: [-1, 5, 10, 20, 30, 60, 120] }, over)

export const rebootSchedule = (over?: Json): Json =>
  merge({ enabled: false, mode: 'weekly', weekday: 2, interval_days: 1, hour: 2, minute: 0, window_hours: 2 }, over)

export const blocklist = (over?: Json): Json => merge({ blocked: [], max: 32, available: true }, over)

export const watchdog = (over?: Json): Json => merge({ enabled: false, host: null, interval_minutes: 2, failures: 3 }, over)
export const firewall = (over?: Json): Json =>
  merge({ upnp: true, dmz_enabled: false, dmz_ip: null, remote_web_access: false, wan_ping: false }, over)
export const portRules = (over?: Json): Json => merge({ forward_enabled: false, mapping_enabled: false, max_per_kind: 20, rules: [] }, over)
export const dhcpBindings = (over?: Json): Json =>
  merge({ enabled: true, max: 10, lan_ip: '192.168.0.1', netmask: '255.255.255.0', bindings: [] }, over)
export const clock = (over?: Json): Json =>
  merge({ local_time: '2026-10-06 22:49:15', utc_offset_hours: 8, mode: 'auto', source: 'NITZ', sntp_synced: false, servers: [] }, over)

export const apnMode = (over?: Json): Json => merge({ apn_mode: 1 }, over)

export function apnProfiles(over?: { apnListArray?: Json[] }): Json {
  return {
    apnListArray: over?.apnListArray ?? [
      { profilename: 'Synthetic Internet', wanapn: 'internet.example', username: '', password: '', pdpType: 3, pppAuthMode: 0, profileId: '1', isEnable: true },
      { profilename: 'Synthetic M2M', wanapn: 'm2m.example', username: '', password: '', pdpType: 1, pppAuthMode: 0, profileId: '2', isEnable: false },
    ],
  }
}

// ── Network / Wi-Fi / router ─────────────────────────────────────────────────

/**
 * Wi-Fi status (agent/src/wifi.rs passthrough). Sanitised shape based on HK B04:
 * htmode EHT40 (2.4 GHz) / EHT80 (5 GHz), actual widths 40 / 80 MHz, TX power
 * as a percentage string from uci. SSIDs and keys are placeholders.
 */
export function wifiStatus(over?: Json): Json {
  return merge(
    {
      wifi_onoff: '1', wifi_onoff_supported: true,
      wifi6_switch: '1', wifi6_supported: true, wifi7_supported: true,
      radio2_disabled: '0', radio5_disabled: '0',
      channel_2g: '0', channel_5g: '44',
      actual_channel_2g: 6, actual_channel_5g: 44,
      actual_bw_2g: '40 MHz', actual_bw_5g: '80 MHz',
      htmode_2g: 'EHT40', htmode_5g: 'EHT80',
      hwmode_2g: '11beg', hwmode_5g: '11bea',
      supported_standards_2g: 'b,g,n,ax,be',
      supported_standards_5g: 'a,n,ac,ax,be',
      bandwidth_options_2g: ['EHT20', 'EHT40'],
      bandwidth_options_5g: ['EHT20', 'EHT40', 'EHT80', 'EHT160'],
      txpower_2g: '80', txpower_5g: '80',
      country_code: 'ZZ',
      ssid_2g: 'Synthetic-WiFi', ssid_5g: 'Synthetic-WiFi',
      key_2g: '', key_5g: '', has_key_2g: true, has_key_5g: true,
      encryption_2g: 'psk3-mixed', encryption_5g: 'psk3-mixed',
      hidden_2g: '0', hidden_5g: '0',
      clients_2g: 1, clients_5g: 2, clients_total: 3,
      guest_ssid: 'synthetic-guest', guest_disabled_2g: '1', guest_disabled_5g: '1',
      guest_encryption: 'none', has_guest_key: false, guest_hidden: '0', guest_active_time: '240',
    },
    over,
  )
}

/** Synthetic clients; MACs use the documentation-style 02:00:5e prefix. */
export function clients(over?: { clients?: Json[] }): Json {
  return {
    clients: over?.clients ?? [
      { mac: '02:00:5E:00:00:01', ip: '192.168.0.101', hostname: 'synthetic-laptop', medium: 'wifi', medium_detail: 'wifi_5ghz', wifi_band: '5 GHz', signal_dbm: -42, tx_bitrate_mbps: 2401.9, rx_bitrate_mbps: 2401.9, expected_throughput_mbps: 1680.0, connected_secs: 184_200 },
      { mac: '02:00:5E:00:00:02', ip: '192.168.0.102', hostname: 'synthetic-phone', medium: 'wifi', medium_detail: 'wifi_2ghz', wifi_band: '2.4 GHz', signal_dbm: -67, tx_bitrate_mbps: 144.4, rx_bitrate_mbps: 115.6, expected_throughput_mbps: 90.0, connected_secs: 402_100 },
      { mac: '02:00:5E:00:00:03', ip: '192.168.0.104', hostname: 'synthetic-usb-host', medium: 'usb-c', medium_detail: 'usb_c', interface: 'ncm0', connected_secs: 7800 },
    ],
  }
}

export function dns(over?: Json): Json {
  return merge(
    {
      prefer_dns_manual: '192.0.2.1',
      standby_dns_manual: '192.0.2.2',
      ipv6_wan_prefer_dns_manual: '2001:db8::1',
      ipv6_wan_standby_dns_manual: '2001:db8::2',
    },
    over,
  )
}

export function lan(over?: Json): Json {
  return merge(
    { ipaddr: '192.168.0.1', netmask: '255.255.255.0', dhcp_enabled: true, dhcp_start: '192.168.0.2', dhcp_end: '192.168.0.253', lease_seconds: 86_400 },
    over,
  )
}

export function ttlStatus(over?: Json): Json {
  return merge({ active: false, ipv6_active: false, ttl_value: 0 }, over)
}

// ── USB ──────────────────────────────────────────────────────────────────────

/** Sanitised shape based on HK B04 (agent/src/usb.rs `usb_status`). */
export function usbStatus(over?: Json): Json {
  return merge(
    {
      mode: 'user',
      active_mode: 'ecm',
      default_mode: 'ecm',
      ncm_persist_on_boot: false,
      supported_modes: ['rndis', 'ecm', 'ncm'],
      experimental_modes: ['ncm'],
      mode_capabilities: [
        { mode: 'rndis', supported: true, experimental: false, function: 'gsi.rndis' },
        { mode: 'ecm', supported: true, experimental: false, function: 'gsi.ecm' },
        { mode: 'ncm', supported: true, experimental: true, function: 'ncm.0', note: 'configfs NCM exists, but the stock ubus USB switch does not expose it' },
      ],
      composition_functions: ['gsi.ecm', 'mass_storage.0'],
      configfs: { present: true, ncm: true, gsi_ecm: true, gsi_rndis: true },
      bridge: { name: 'br-lan', members: ['ecm0', 'wlan0', 'wlan2', 'ncm0'] },
      interfaces: { ecm0: true, rndis0: false, ncm0: true, ncm_ifname: null },
      usb_ids: { vendor: '0x19d2', product: '0x1405' },
      connect: 1,
      typec_cc: 'cc1',
      link: {
        negotiated: 'super-speed', negotiated_label: 'USB 3.0', negotiated_mbps: 5000,
        max: 'super-speed-plus', max_label: 'USB 3.1 Gen2', max_mbps: 10_000, at_full_speed: false,
      },
    },
    over,
  )
}

// ── System ───────────────────────────────────────────────────────────────────

/** Mirrors agent/src/system.rs::ProcessListResult. Process names are stock daemon names. */
export function top(over?: Json): Json {
  const processes = [
    { pid: 487, name: 'zte_topsw_tr069', cpu_pct: 3.2, rss_kb: 23_300, state: 'sleeping', is_bloat: true },
    { pid: 611, name: 'zte_router', cpu_pct: 2.4, rss_kb: 9800, state: 'sleeping', is_bloat: false },
    { pid: 512, name: 'zte_mqtt_sdk_st', cpu_pct: 1.8, rss_kb: 11_100, state: 'sleeping', is_bloat: true },
    { pid: 811, name: 'zte-agent', cpu_pct: 0.6, rss_kb: 2048, state: 'running', is_bloat: false },
  ]
  return merge(
    {
      processes,
      total_count: 142,
      bloat_count: 2,
      bloat_cpu_pct: 5.0,
      bloat_rss_kb: 34_400,
    },
    over,
  )
}

export const atPort = (over?: Json): Json => merge({ port: '/dev/at_mdm0', available: true }, over)

/** Logger status (signal and connection loggers share this shape). */
export function loggerStatus(over?: Json): Json {
  return merge(
    { running: false, samples: 0, events: 0, elapsed_secs: 0, duration_secs: 3600, interval_secs: 3, last_error: null, max_bytes: 1_048_576, flush_interval_secs: 30 },
    over,
  )
}

export const signalLogCsv = (): string =>
  'timestamp,datetime,network_type,carrier,cell_id,lte_band,lte_pci,lte_earfcn,lte_rsrp,lte_rsrq,lte_sinr,lte_rssi,nr_band,nr_pci,nr_arfcn,nr_rsrp,nr_rsrq,nr_sinr,nr_rssi,lte_ca_bands,nr_ca_bands\n' +
  '1790000000,2026-10-01T09:20:00,SA,Synthetic Carrier,5000000123,,0,0,-48,-7,21.0,,n78,745,643392,-53,-11,31.0,,,\n'

export const connectionLogCsv = (): string =>
  'timestamp,datetime,event_type,detail,old_value,new_value\n' +
  '1790000012,2026-10-01T09:20:12,nr_band_change,NR band changed,n78,n41\n'

export const killBloatResult = (over?: Json): Json =>
  merge({ killed: [{ pid: 487, name: 'zte_topsw_tr069' }], skipped: [], freed_rss_kb: 23_300 }, over)

export const atSendResult = (command = 'AT', over?: Json): Json =>
  merge({ command, response: `${command}\r\nOK`, port: '/dev/at_mdm0', elapsed_ms: 42 }, over)
