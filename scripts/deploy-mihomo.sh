#!/bin/bash
# Install or update the mihomo core and its rule data on the modem over SSH.
#
#   bash scripts/deploy-mihomo.sh [--gateway ADDRESS] [--dry-run]
#
# Downloads a pinned mihomo release, the latest MetaCubeX geodata and a pinned
# mainland IPv4 list (for the TUN's mainland bypass) on this computer, verifies
# every file against a published or pinned SHA-256, then
# stages each file in /data/mihomo, re-checks the hash on the device and moves
# it into place. Nothing outside /data/mihomo is touched; the agent manages the
# process (Proxy page). A running mihomo keeps the old binary until restarted.
set -euo pipefail

MIHOMO_VERSION=v1.19.32
# MetaCubeX/meta-rules-dat geo/geoip/cn.list at a fixed commit (bump both together).
CN_LIST_COMMIT=989c8194a8c01e8d85ab2c33aa7d8af3849d3393
CN_LIST_SHA256=1c1b257518487ab565e9c91657526b417a2983fe353c4f50a013372aff2e1f18
GATEWAY="${ZTE_GATEWAY:-192.168.0.1}"
DRY_RUN=0
while [ $# -gt 0 ]; do
    case "$1" in
        --gateway) GATEWAY="$2"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        *) echo "usage: $0 [--gateway ADDRESS] [--dry-run]" >&2; exit 64 ;;
    esac
done

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$HOME/.ssh/known_hosts.d"
SSH=(ssh -p 2222 -o BatchMode=yes -o LogLevel=ERROR -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new
     -o "UserKnownHostsFile=$HOME/.ssh/known_hosts.d/zte" "root@$GATEWAY")

fetch() { curl --fail --silent --show-error --location --proto '=https' --max-time 300 -o "$2" "$1"; }
sha() { shasum -a 256 "$1" | awk '{print $1}'; }

# GitHub's per-asset digest for a release, as "sha256:<hex>".
asset_digest() { # repo tag asset
    fetch "https://api.github.com/repos/$1/releases/tags/$2" "$WORK/release.json"
    python3 - "$WORK/release.json" "$3" <<'PY'
import json, sys
release = json.load(open(sys.argv[1]))
for asset in release.get("assets", []):
    if asset["name"] == sys.argv[2] and str(asset.get("digest", "")).startswith("sha256:"):
        print(asset["digest"][7:])
        break
else:
    sys.exit(f"no sha256 digest published for {sys.argv[2]}")
PY
}

echo "Downloading mihomo $MIHOMO_VERSION…"
CORE_ASSET="mihomo-linux-arm64-$MIHOMO_VERSION.gz"
fetch "https://github.com/MetaCubeX/mihomo/releases/download/$MIHOMO_VERSION/$CORE_ASSET" "$WORK/$CORE_ASSET"
[ "$(sha "$WORK/$CORE_ASSET")" = "$(asset_digest MetaCubeX/mihomo "$MIHOMO_VERSION" "$CORE_ASSET")" ] \
    || { echo "mihomo download failed verification" >&2; exit 1; }
gzip -dc "$WORK/$CORE_ASSET" > "$WORK/mihomo"
head -c 20 "$WORK/mihomo" | od -An -tx1 | tr -d ' \n' | grep -q '^7f454c460201' \
    || { echo "mihomo is not a 64-bit ELF" >&2; exit 1; }

echo "Downloading geodata…"
GEO_BASE=https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest
for file in geoip.metadb geoip.dat geosite.dat; do
    fetch "$GEO_BASE/$file" "$WORK/$file"
    fetch "$GEO_BASE/$file.sha256sum" "$WORK/$file.sha256sum"
    [ "$(sha "$WORK/$file")" = "$(awk '{print $1}' "$WORK/$file.sha256sum")" ] \
        || { echo "$file failed verification" >&2; exit 1; }
done
fetch "https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/$CN_LIST_COMMIT/geo/geoip/cn.list" "$WORK/cn.list"
[ "$(sha "$WORK/cn.list")" = "$CN_LIST_SHA256" ] || { echo "cn.list failed verification" >&2; exit 1; }
echo "All downloads verified."

if [ "$DRY_RUN" = 1 ]; then
    echo "Dry run: nothing was copied to the modem."
    exit 0
fi

"${SSH[@]}" 'mkdir -p /data/mihomo/providers && chmod 700 /data/mihomo'
for file in mihomo geoip.metadb geoip.dat geosite.dat cn.list; do
    want=$(sha "$WORK/$file")
    "${SSH[@]}" "set -e; cat > /data/mihomo/$file.new; \
        test \"\$(sha256sum /data/mihomo/$file.new | awk '{print \$1}')\" = $want; \
        chmod 700 /data/mihomo/$file.new; mv -f /data/mihomo/$file.new /data/mihomo/$file" < "$WORK/$file"
    echo "  installed $file"
done
"${SSH[@]}" '/data/mihomo/mihomo -v | head -1'
echo "Done. If mihomo was running, restart it from the dashboard (Proxy → Restart) to use the new core."
