#!/bin/sh
# net_time_guard.sh — keep two firmware-reverted settings in place.
#
# 1. LAN IPv6: the carrier gives a single /64 with no prefix delegation, so
#    odhcpd must relay RA/DHCPv6 from zte_wan6 to the LAN. zte_router
#    rewrites dhcp.lan back to ra/dhcpv6=server on every boot, which leaves
#    clients without a global IPv6. With IPA and the mihomo TUN also on, the
#    agent keeps TUN'd IPv6 untracked so IPA offload cannot stall it.
#    The /64 is also routed on-link to br-lan. odhcpd's learned /128s alone
#    are not enough: the kernel drops an idle neighbour after about a minute
#    (gc_thresh1=1 here), odhcpd then drops its route, and replies to that
#    client follow the default route back out the WAN. NDP relay stays off
#    on zte_wan6. The WAN is raw IP, so the pings odhcpd sends there for
#    every LAN NS leave NOARP neighbours that it takes for upstream hosts; it
#    then routed those clients to the WAN and answered NS (DAD too) for them
#    on br-lan (2026-10-30). Only the router is advertised as the IPv6
#    resolver, so every LAN lookup goes through dnsmasq (2026-11-04).
# 2. Clock: ZTE's NITZ time sync writes Beijing wall time into the kernel
#    clock while the OS timezone is UTC, so the epoch runs 8h fast. The OS
#    timezone is CST-8 now; this keeps the kernel clock on true UTC using the
#    Date header of a domestic HTTP server (no ntpd on this firmware).
#
# Runs from rc.local; survives via nohup. Logs to /tmp/net_time_guard.log
# (rotated at 32KB).

LOG=/tmp/net_time_guard.log
BOOT_DELAY=90             # let zte_router finish its boot-time rewrites
CHECK_SECS=300
ROUTE_SECS=15             # re-check the LAN /64 route (tracks a new WAN prefix)
V6_PROTO=99               # route protocol tag of the /64 this script owns
MAX_SKEW=60
MAX_LOG=32768
TIME_URLS="http://www.baidu.com http://www.qq.com http://www.aliyun.com"

log() { echo "$(date '+%F %T') $*" >> "$LOG"; [ -s "$LOG" ] && [ "$(wc -c < "$LOG")" -gt $MAX_LOG ] && tail -c $((MAX_LOG / 2)) "$LOG" > "$LOG.new" 2>/dev/null && mv "$LOG.new" "$LOG"; }

fix_v6_relay() {
    # Do not commit another caller's staged DHCP changes.
    [ -z "$(uci -q changes dhcp)" ] || return 0
    changed=0
    want() { [ "$(uci -q get "$1")" = "$2" ] || { uci set "$1=$2"; changed=1; }; }
    want dhcp.wan_v6.interface zte_wan6
    want dhcp.wan_v6.master 1
    want dhcp.wan_v6.ndp disabled
    for o in ra dhcpv6; do
        want dhcp.wan_v6.$o relay
    done
    for o in ra dhcpv6 ndp; do
        want dhcp.lan.$o relay
    done
    # Advertise only the router as the LAN's IPv6 resolver. The firmware
    # lists the carrier's resolvers here too, and clients that pick those
    # bypass dnsmasq (and mihomo behind it) entirely.
    ll=$(ip -6 addr show dev br-lan scope link 2>/dev/null |
        awk '$1 == "inet6" { split($2, a, "/"); print a[1]; exit }')
    if [ -n "$ll" ] && [ "$(uci -q get dhcp.lan.dns)" != "$ll" ]; then
        uci -q delete dhcp.lan.dns
        uci add_list dhcp.lan.dns="$ll"
        changed=1
    fi
    if [ $changed = 1 ]; then
        uci commit dhcp
        /etc/init.d/odhcpd restart >/dev/null 2>&1
        log "dhcp IPv6 relay re-applied"
    fi
}

fix_v6_route() {
    wan=$(ifstatus zte_wan6 2>/dev/null | jsonfilter -e '@.l3_device' 2>/dev/null)
    [ -n "$wan" ] || return 0
    pfx=$(ip -6 addr show dev "$wan" scope global -deprecated -tentative 2>/dev/null |
        awk '$1 == "inet6" && $2 ~ /\/64$/ { print $2; exit }')
    if [ -z "$pfx" ]; then
        ip -6 route flush dev br-lan proto $V6_PROTO 2>/dev/null
        return 0
    fi
    # $pfx keeps the WAN address's host bits; ip masks them for both calls.
    if [ -z "$(ip -6 route show exact "$pfx" dev br-lan proto $V6_PROTO)" ]; then
        ip -6 route flush dev br-lan proto $V6_PROTO 2>/dev/null
        ip -6 route add "$pfx" dev br-lan proto $V6_PROTO \
            && log "LAN IPv6 $(ip -6 route show dev br-lan proto $V6_PROTO | awk '{ print $1; exit }') routed on-link"
    fi
    # Undo what odhcpd learned while NDP relay was on for the WAN: /128s to
    # LAN clients via the WAN, and proxy entries for them. odhcpd ignores
    # events on the WAN now, so it would never remove them itself.
    ip -6 route show root "$pfx" dev "$wan" 2>/dev/null | while read -r dst _; do
        case $dst in */*) ;; *) ip -6 route del "$dst" dev "$wan" ;; esac
    done
    for dev in br-lan "$wan"; do
        ip -6 neigh show proxy to "$pfx" dev "$dev" 2>/dev/null | while read -r dst _; do
            ip -6 neigh del proxy "$dst" dev "$dev"
        done
    done
}

fix_clock() {
    for url in $TIME_URLS; do
        hdr=$(curl -sI -m 5 "$url" 2>/dev/null | tr -d '\r' | sed -n 's/^[Dd]ate: //p' | head -1)
        [ -n "$hdr" ] || continue
        real=$(date -u -D "%a, %d %b %Y %H:%M:%S GMT" -d "$hdr" +%s 2>/dev/null) || continue
        now=$(date +%s)
        skew=$((now - real))
        [ ${skew#-} -le $MAX_SKEW ] && return 0
        date -u -s "$(date -u -D "%a, %d %b %Y %H:%M:%S GMT" -d "$hdr" '+%Y-%m-%d %H:%M:%S')" >/dev/null 2>&1 \
            && log "clock skew ${skew}s corrected from $url"
        return 0
    done
    log "clock check failed: no HTTP Date header"
}

# musl here reads /etc/localtime (-> /tmp/localtime) and ignores /etc/TZ, and
# the firmware ships no zoneinfo; link our copied TZif before daemons start.
[ -f /data/zoneinfo/Asia/Shanghai ] && ln -sf /data/zoneinfo/Asia/Shanghai /tmp/localtime

sleep $BOOT_DELAY
n=0
while :; do
    if [ $((n % (CHECK_SECS / ROUTE_SECS))) = 0 ]; then
        fix_v6_relay
        fix_clock
    fi
    fix_v6_route
    n=$((n + 1))
    sleep $ROUTE_SECS
done
