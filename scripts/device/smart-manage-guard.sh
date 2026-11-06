#!/bin/sh
# smart_manage_guard.sh — restart zte_smart_manage when it bloats.
#
# zte_smart_manage (ZTE DPI/game-accel engine) leaks ~8MB/h of private dirty
# memory; after ~3 days uptime it holds 600MB+ RSS on a 1.5GB device and the
# whole router starts to stall. Restarting it via its procd init script is
# safe (SAFETY.md: it is a daemon.conf barrier member, so never kill -9 and
# never init.d-disable it; /etc/init.d/zte_smart_manage restart is the
# sanctioned path). After restart it settles at ~5MB RSS.
#
# Runs from rc.local; survives via nohup. Logs to /tmp/smart_manage_guard.log
# (rotated at 32KB). MemAvailable guard: if the system is already tight we
# still restart, since the restart itself frees memory.

LOG=/tmp/smart_manage_guard.log
THRESH_KB=262144          # 256MB RSS trigger
CHECK_SECS=600            # sample every 10 min
MIN_AGE_SECS=600          # ignore processes younger than 10 min
MAX_LOG=32768

log() { echo "$(date '+%F %T') $*" >> "$LOG"; [ -s "$LOG" ] && [ "$(wc -c < "$LOG")" -gt $MAX_LOG ] && tail -c $((MAX_LOG / 2)) "$LOG" > "$LOG.new" 2>/dev/null && mv "$LOG.new" "$LOG"; }

while :; do
    sleep $CHECK_SECS
    PID=$(pidof zte_smart_manage) || continue
    [ -n "$PID" ] || continue
    RSS=$(awk '/^VmRSS/{print $2}' "/proc/$PID/status" 2>/dev/null)
    [ -n "$RSS" ] || continue
    if [ "$RSS" -gt $THRESH_KB ]; then
        # skip freshly restarted instances (procd respawn churn guard)
        BOOT=$(awk '{print $1}' /proc/uptime | cut -d. -f1)
        START=$(awk '{print int($22/100)}' "/proc/$PID/stat" 2>/dev/null)
        AGE=$((BOOT - START))
        [ "$AGE" -lt $MIN_AGE_SECS ] && continue
        log "rss ${RSS}KB > ${THRESH_KB}KB (age ${AGE}s), restarting"
        /etc/init.d/zte_smart_manage restart >/dev/null 2>&1
        sleep 5
        NP=$(pidof zte_smart_manage)
        NRSS=$(awk '/^VmRSS/{print $2}' "/proc/$NP/status" 2>/dev/null)
        log "restarted: new pid $NP rss ${NRSS}KB av $(awk '/MemAvailable/{print $2}' /proc/meminfo)KB"
    fi
done
