#!/bin/sh
# band_guard.sh — keep the modem on the fast 5G layer and fall back by itself.
#
# Why: at the owner's desk the carrier (China Mobile) parks the modem on n28
# (700 MHz, 30 MHz wide: 10-34 Mbps down, 450 ms pings and loss as soon as
# one download fills it) because n41/n79 are weak there (RSRP -102/-108),
# although n41/n79 give 100-190 Mbps and hold a session without loss
# (measured 2026-11-17). With every band allowed the network moved the modem
# back to n28 within about five minutes. Which band serves is the base
# station's decision; the only device-side lever is the NR SA band list.
#
# The ladder, best first:
#   1. PREF bands only (n41+n79). The network adds carriers there on its own;
#      the "5GA" icon is n41+n79 aggregation (>= 200 MHz) and cannot be forced.
#   2. Every NR band (adds n28) when the modem cannot stay on PREF.
#   3. LTE. That is the modem's own fallback and needs network mode
#      WL_AND_5G (5G/4G/3G auto), which this script never changes.
# It climbs back to 1 on a timer with backoff. Changing the SA band list
# keeps the data session (0 s outage and the same WAN address in every switch
# measured), so trying is cheap where PREF exists; where it does not, a try
# costs up to TRY_SECS without 5G, hence the backoff.
#
# Deference: a band list that is neither PREF nor the full list was chosen by
# hand (dashboard), and the guard leaves it alone until it is the full list
# again. To switch the guard off: touch /data/local/tmp/band_guard.off
#
# Only zte_nwinfo_api netinfo/bandlock is used (docs/SAFETY.md, known-good
# ubus surface). Never nwinfo_set_mode: the modem does not come back from
# low-power mode without a reboot.
#
# Runs from rc.local; survives via nohup. Logs changes to /tmp/band_guard.log
# (rotated at 32KB); current state in /tmp/band_guard.state.

PREF="${BAND_GUARD_PREF:-41,79}"
DIR=/data/local/tmp
FULL_FILE=$DIR/band_guard.full        # the device's own unlocked list, saved at install
OFF_FILE=$DIR/band_guard.off
FULL_DEFAULT="1,2,3,5,7,8,18,20,26,28,29,38,40,41,48,66,71,75,77,78,79"
LOG=/tmp/band_guard.log
STATE=/tmp/band_guard.state
LOCKF=/tmp/band_guard.lock
BOOT_DELAY="${BAND_GUARD_BOOT_DELAY:-90}"   # let the modem register first
CHECK_SECS=10
DOWN_CHECKS=2             # this many checks off PREF -> allow every band
WEAK_RSRP=-116            # dBm; at or below this for WEAK_CHECKS checks -> allow every band
WEAK_CHECKS=3
TRY_SECS=10               # how long PREF gets to register when climbing back
RETRY_MIN="${BAND_GUARD_RETRY_MIN:-180}"
RETRY_MAX=1800
HOLD_OK_SECS=300          # a stay on PREF this long resets the backoff
MAX_LOG=32768

log() { echo "$(date '+%F %T') $*" >> "$LOG"; [ -s "$LOG" ] && [ "$(wc -c < "$LOG")" -gt $MAX_LOG ] && tail -c $((MAX_LOG / 2)) "$LOG" > "$LOG.new" 2>/dev/null && mv "$LOG.new" "$LOG"; }
note() { [ "$1" = "$NOTE" ] || { NOTE=$1; log "$1"; }; }
up() { cut -d. -f1 /proc/uptime; }    # the wall clock jumps on this firmware
cur_lock() { uci -q get zte_nwinfo.band_lock.nr5g_sa_band_lock; }

read_net() {
    J=$(ubus call zte_nwinfo_api nwinfo_get_netinfo 2>/dev/null)
    TYPE=$(jsonfilter -s "$J" -e '@.network_type' 2>/dev/null)
    BAND=$(jsonfilter -s "$J" -e '@.wan_active_band' 2>/dev/null)
    RSRP=$(jsonfilter -s "$J" -e '@.nr5g_rsrp' 2>/dev/null)
}

on_pref() {   # registered on 5G SA, on one of the PREF bands
    case "$TYPE" in SA|NR5G-SA) ;; *) return 1 ;; esac
    [ -n "$BAND" ] || return 1
    case ",$PREF," in *",${BAND#n},"*) return 0 ;; esac
    return 1
}

connected() {   # the data call is up
    case "$(ubus call zwrt_data get_wwaniface '{"source_module":"zte_topsw_data","cid":1}' 2>/dev/null | jsonfilter -e '@.connect_status' 2>/dev/null)" in
        *disconnect*) return 1 ;;
        *connected*) return 0 ;;
    esac
    return 1
}

set_lock() {   # $1 = band list; confirmed from uci
    n=0
    while [ $n -lt 3 ]; do
        ubus call zte_nwinfo_api nwinfo_set_nrbandlock "{\"nr5g_type\":\"SA\",\"nr5g_band\":\"$1\"}" >/dev/null 2>&1
        i=0
        while [ $i -lt 4 ]; do
            [ "$(cur_lock)" = "$1" ] && return 0
            sleep 1
            i=$((i + 1))
        done
        n=$((n + 1))
    done
    return 1
}

later() {   # schedule the next climb, then lengthen the one after it
    NEXT_TRY=$(($(up) + BACKOFF))
    WAIT=$BACKOFF
    BACKOFF=$((BACKOFF * 2))
    [ $BACKOFF -gt $RETRY_MAX ] && BACKOFF=$RETRY_MAX
}

release() {   # $1 = reason; PREF -> every band
    held=$(($(up) - HIGH_SINCE))
    [ $held -ge $HOLD_OK_SECS ] && BACKOFF=$RETRY_MIN
    set_lock "$FULL" || log "could not restore the full band list"
    later
    BAD=0
    WEAK=0
    log "left bands $PREF after ${held}s ($1); every band allowed, next try in ${WAIT}s"
}

climb() {   # every band -> PREF, kept only if the modem registers there
    from="${TYPE:-no service} $BAND"
    if ! set_lock "$PREF"; then
        set_lock "$FULL"
        later
        log "band lock was not accepted; next try in ${WAIT}s"
        return 1
    fi
    t=0
    while [ $t -lt $TRY_SECS ]; do
        sleep 2
        t=$((t + 2))
        read_net
        if on_pref && connected; then
            HIGH_SINCE=$(up)
            BAD=0
            WEAK=0
            log "on $BAND (rsrp $RSRP) with only bands $PREF allowed; was $from"
            return 0
        fi
    done
    set_lock "$FULL" || log "could not restore the full band list"
    later
    log "bands $PREF did not register in ${TRY_SECS}s (${TYPE:-no service} $BAND); every band allowed, next try in ${WAIT}s"
    return 1
}

trap '' HUP
exec 9>"$LOCKF"
flock -n 9 || exit 0      # one instance

sleep "$BOOT_DELAY"
if [ -s "$FULL_FILE" ]; then FULL=$(cat "$FULL_FILE"); else FULL=$FULL_DEFAULT; fi
BACKOFF=$RETRY_MIN
NEXT_TRY=0
BAD=0
WEAK=0
NOTE=
HIGH_SINCE=$(up)
log "started: prefer bands $PREF, otherwise $FULL"

while :; do
    if [ -e "$OFF_FILE" ]; then
        note "switched off by $OFF_FILE"
        sleep $CHECK_SECS
        continue
    fi
    LOCK=$(cur_lock)
    read_net
    if [ "$LOCK" = "$PREF" ]; then
        MODE=pref
        NOTE=
        if on_pref; then
            BAD=0
            if [ "$RSRP" -lt 0 ] 2>/dev/null && [ "$RSRP" -le $WEAK_RSRP ]; then WEAK=$((WEAK + 1)); else WEAK=0; fi
            [ $WEAK -ge $WEAK_CHECKS ] && release "rsrp $RSRP on $BAND"
        else
            BAD=$((BAD + 1))
            [ $BAD -ge $DOWN_CHECKS ] && release "now ${TYPE:-no service} $BAND"
        fi
    elif [ "$LOCK" = "$FULL" ]; then
        MODE=all
        NOTE=
        if on_pref || [ "$(up)" -ge $NEXT_TRY ]; then
            connected && climb
        fi
    else
        MODE=manual
        note "band list $LOCK was set by hand; leaving it alone"
    fi
    left=$((NEXT_TRY - $(up)))
    [ $left -lt 0 ] && left=0
    echo "$(date '+%F %T') mode=$MODE lock=$(cur_lock) type=$TYPE band=$BAND rsrp=$RSRP next_try_in=${left}s" > "$STATE"
    sleep $CHECK_SECS
done
