#!/bin/sh
# net_time_guard.sh — keep two firmware-reverted settings in place.
#
# 1. LAN IPv6: the carrier gives a single /64 with no prefix delegation, so
#    odhcpd must relay RA/DHCPv6/NDP from zte_wan6 to the LAN. zte_router
#    rewrites dhcp.lan back to ra/dhcpv6=server on every boot, which leaves
#    clients without a global IPv6. With IPA and the mihomo TUN also on, the
#    agent keeps TUN'd IPv6 untracked so IPA offload cannot stall it.
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
    for o in ra dhcpv6 ndp; do
        want dhcp.wan_v6.$o relay
        want dhcp.lan.$o relay
    done
    if [ $changed = 1 ]; then
        uci commit dhcp
        /etc/init.d/odhcpd restart >/dev/null 2>&1
        log "dhcp IPv6 relay re-applied"
    fi
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
while :; do
    fix_v6_relay
    fix_clock
    sleep $CHECK_SECS
done
