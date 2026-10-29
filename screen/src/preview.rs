//! `--preview DIR`: render every page from a plausible synthetic model to PNG,
//! for reviewing the design without hardware.

use std::fs;
use std::path::Path;

use crate::canvas::Canvas;
use crate::settings::{Settings, Theme};
use crate::stats::{
    push, Battery, Client, Iface, Model, Proc, Proxy, Radio, Station, WifiBand, SERVICES,
};
use crate::ui::{Page, Ui, SPLASH_FRAMES};

pub fn sample() -> Model {
    let mut m = Model {
        clock: "21:37:42".into(),
        date: "2026-10-28".into(),
        kernel: "5.15.194-perf".into(),
        firmware: "CN_ZTE_MU5250V1.0.0B31".into(),
        cpu: vec![23.4, 8.1, 1.2, 5.0],
        freq_mhz: vec![2208, 1516, 1516, 1516],
        governor: "schedutil".into(),
        mem_total_kb: 1_628_400,
        mem_avail_kb: 742_700,
        mem_cached_kb: 382_800,
        load: [0.41, 0.36, 0.52],
        tasks: 384,
        running: 2,
        ctxt_rate: 3240.0,
        intr_rate: 5120.0,
        uptime_s: 15_133,
        temp_cpu: Some(49.0),
        temp_modem: Some(47.0),
        temp_board: Some(43.7),
        battery: Battery {
            percent: Some(100),
            status: "Full".into(),
            health: "Good".into(),
            voltage_mv: Some(4472),
            current_ma: Some(-4),
            temp_c: Some(38.0),
            external_power: true,
            cycles: Some(6),
            wear: Some(97.3),
            ..Default::default()
        },
        zones: vec![
            ("cpuss".into(), 49.4),
            ("mdmq6".into(), 49.0),
            ("aoss".into(), 47.4),
            ("mdmss".into(), 47.0),
            ("xo-therm".into(), 43.7),
            ("battery".into(), 38.0),
            ("sdr0".into(), 27.0),
        ],
        day_rx: Some(3_221_225_472),
        day_tx: Some(402_653_184),
        month_rx: Some(48_318_382_080),
        month_tx: Some(6_442_450_944),
        wifi: vec![
            WifiBand {
                label: "2.4G",
                ssid: "u60-home".into(),
                channel: "6".into(),
                width: "20M".into(),
                txpower: "19dBm".into(),
                stations: 2,
            },
            WifiBand {
                label: "5G",
                ssid: "u60-home".into(),
                channel: "44".into(),
                width: "160M".into(),
                txpower: "18dBm".into(),
                stations: 1,
            },
        ],
        disks: vec![("data", 14.0), ("tmp", 0.2), ("etc", 4.0)],
        services: SERVICES.iter().map(|(l, _)| (*l, true)).collect(),
        conntrack: Some(272),
        conntrack_max: Some(65536),
        tcp_inuse: Some(144),
        clients: Some(4),
        sta_2g: Some(2),
        sta_5g: Some(1),
        radio: Some(Radio {
            rat: "SA".into(),
            band: "n41".into(),
            bandwidth: "100".into(),
            pci: Some(843),
            arfcn: Some(504_990),
            operator: "China Mobile".into(),
            roaming: false,
            rsrp: Some(-57.0),
            rsrq: Some(-11.0),
            sinr: Some(36.9),
            rssi: Some(-51.0),
            bars: Some(5),
            ca: String::new(),
            mode: "WL_AND_5G".into(),
        }),
        dl: Some(1_250_000.0),
        ul: Some(310_000.0),
        peak_dl: 11_896_530.0,
        peak_ul: 3_648_909.0,
        session_rx: Some(828_820_212),
        session_tx: Some(94_269_514),
        session_s: Some(13_375),
        wan_ip: Some("10.56.99.1".into()),
        wan6_up: Some(true),
        dns: vec![
            "218.202.4.4".into(),
            "218.202.3.3".into(),
            "2409:8088::a".into(),
        ],
        ifaces: vec![
            Iface {
                label: "wan",
                up: true,
                rx_rate: 52_000.0,
                tx_rate: 9_000.0,
            },
            Iface {
                label: "lan",
                up: true,
                rx_rate: 310_000.0,
                tx_rate: 1_250_000.0,
            },
            Iface {
                label: "wifi2g",
                up: true,
                rx_rate: 120_000.0,
                tx_rate: 800_000.0,
            },
            Iface {
                label: "wifi5g",
                up: true,
                rx_rate: 190_000.0,
                tx_rate: 450_000.0,
            },
            Iface {
                label: "usb",
                up: true,
                rx_rate: 0.0,
                tx_rate: 0.0,
            },
            Iface {
                label: "tun",
                up: true,
                rx_rate: 41_000.0,
                tx_rate: 12_000.0,
            },
        ],
        mihomo: Some(Proc {
            pid: 17969,
            name: "mihomo".into(),
            cpu: 3.5,
            rss_kb: 119_656,
        }),
        proxy: Some(Proxy {
            mode: "rule".into(),
            connections: 37,
            down_total: 1_288_490_188,
            up_total: 314_572_800,
        }),
        live: crate::proxy::View::sample_live(),
        ..Default::default()
    };
    let st = |band, signal, rate| {
        Some(Station {
            mac: String::new(),
            band,
            signal: Some(signal),
            rate_mbit: Some(rate),
            connected_s: Some(600),
        })
    };
    m.lan = vec![
        Client {
            host: "macbook-pro".into(),
            ip: "192.168.0.149".into(),
            station: st("5G", -44, 1201.0),
        },
        Client {
            host: "pixel-9".into(),
            ip: "192.168.0.181".into(),
            station: st("2.4G", -61, 86.7),
        },
        Client {
            host: "LAPTOP-OFFICE".into(),
            ip: "192.168.0.197".into(),
            station: st("2.4G", -76, 26.0),
        },
        Client {
            host: "nas".into(),
            ip: "192.168.0.166".into(),
            station: None,
        },
    ];
    m.top = vec![
        Proc {
            pid: 26632,
            name: "zte_topsw_data".into(),
            cpu: 6.2,
            rss_kb: 11_376,
        },
        Proc {
            pid: 17969,
            name: "mihomo".into(),
            cpu: 3.5,
            rss_kb: 119_656,
        },
        Proc {
            pid: 31002,
            name: "zte-screen".into(),
            cpu: 2.1,
            rss_kb: 3_120,
        },
        Proc {
            pid: 8797,
            name: "syslogd".into(),
            cpu: 0.9,
            rss_kb: 1_476,
        },
        Proc {
            pid: 9294,
            name: "zte-agent".into(),
            cpu: 0.4,
            rss_kb: 6_780,
        },
        Proc {
            pid: 31657,
            name: "qcrilNrd".into(),
            cpu: 0.2,
            rss_kb: 24_852,
        },
        Proc {
            pid: 3959,
            name: "QCMAP_ConnectionManager".into(),
            cpu: 0.1,
            rss_kb: 12_984,
        },
    ];
    for i in 0..60 {
        let t = i as f32;
        push(
            &mut m.cpu_hist,
            9.0 + 6.0 * (t / 5.0).sin() + if i % 17 == 0 { 30.0 } else { 0.0 },
        );
        push(
            &mut m.rsrp_hist,
            -60.0 + 4.0 * (t / 7.0).sin() - if (40..46).contains(&i) { 18.0 } else { 0.0 },
        );
        push(
            &mut m.sinr_hist,
            33.0 + 3.0 * (t / 4.0).cos() - if (40..46).contains(&i) { 15.0 } else { 0.0 },
        );
        push(
            &mut m.dl_hist,
            (900_000.0 + 700_000.0 * (t / 6.0).sin()).max(0.0)
                + if (20..28).contains(&i) {
                    1_200_000.0
                } else {
                    0.0
                },
        );
        push(
            &mut m.ul_hist,
            (200_000.0 + 150_000.0 * (t / 3.0).cos()).max(0.0),
        );
        push(&mut m.current_hist, -4.0 + 30.0 * (t / 9.0).sin().max(0.0));
    }
    m
}

pub fn write_all(dir: &Path) -> i32 {
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!("{}: {e}", dir.display());
        return 1;
    }
    let m = sample();
    let mut c = Canvas::new();
    let mut shots: Vec<(String, Ui)> = Vec::new();
    for page in Page::ALL {
        let mut ui = Ui::new(Settings::default());
        ui.splash = SPLASH_FRAMES;
        ui.blink = true;
        ui.page = page;
        shots.push((format!("{page:?}").to_lowercase(), ui));
    }
    let mut splash = Ui::new(Settings::default());
    splash.splash = 2;
    shots.push(("splash".into(), splash));
    let mut conf = Ui::new(Settings::default());
    conf.splash = SPLASH_FRAMES;
    conf.settings_open = true;
    shots.push(("settings".into(), conf));
    for theme in [Theme::Amber, Theme::Cyan] {
        let mut ui = Ui::new(Settings {
            theme,
            ..Settings::default()
        });
        ui.splash = SPLASH_FRAMES;
        ui.page = Page::Sys;
        shots.push((format!("sys-{}", theme.name()), ui));
    }
    let mut idle = Ui::new(Settings::default());
    idle.splash = SPLASH_FRAMES;
    idle.idle_left = Some(7);
    idle.page = Page::Cell;
    shots.push(("cell-idle".into(), idle));
    for scene in ["talk", "think", "pat", "angry", "doze", "day", "dusk"] {
        let mut ui = Ui::new(Settings::default());
        ui.splash = SPLASH_FRAMES;
        ui.blink = true;
        ui.pet = crate::pet::Pet::demo(scene, &m);
        shots.push((format!("pet-{scene}"), ui));
    }
    for screen in ["main", "group", "conns", "subs", "confirm"] {
        let mut ui = Ui::new(Settings::default());
        ui.splash = SPLASH_FRAMES;
        ui.page = Page::Proxy;
        ui.proxy = crate::proxy::View::demo(screen);
        shots.push((format!("proxy-{screen}"), ui));
    }
    // Every scene in each of its lights.
    for place in ["bedroom", "train", "server", "cafe", "library", "rooftop"] {
        for (light, hour) in [("golden", 6), ("day", 12), ("night", 21)] {
            let mut ui = Ui::new(Settings::default());
            ui.splash = SPLASH_FRAMES;
            ui.blink = true;
            ui.pet = crate::pet::Pet::demo(&format!("{place}@{hour}"), &m);
            shots.push((format!("scene-{place}-{light}"), ui));
        }
    }

    for (name, ui) in &shots {
        ui.render(&mut c, &m);
        let path = dir.join(format!("{name}.png"));
        if let Err(e) = fs::write(&path, crate::png::encode(&c)) {
            eprintln!("{}: {e}", path.display());
            return 1;
        }
    }
    // An unhealthy device, to review the alert styling.
    let mut sick = sample();
    sick.temp_cpu = Some(71.0);
    sick.services[3].1 = false;
    sick.services[0].1 = false;
    sick.battery.percent = Some(14);
    sick.battery.external_power = false;
    let mut ui = Ui::new(Settings::default());
    ui.splash = SPLASH_FRAMES;
    ui.blink = true;
    ui.render(&mut c, &sick);
    let _ = fs::write(dir.join("home-alerts.png"), crate::png::encode(&c));
    println!("wrote {} previews to {}", shots.len() + 1, dir.display());
    0
}
