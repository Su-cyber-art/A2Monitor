#!/bin/sh
set -eu
SERVER= TOKEN= INTERVAL=5 IFACE= PREFIX=/opt/monitor SERVICE=monitor-agent BINARY_URL= UNINSTALL=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --server) SERVER=$2; shift 2;; --token) TOKEN=$2; shift 2;;
    --interval) INTERVAL=$2; shift 2;; --iface) IFACE=$2; shift 2;;
    --prefix) PREFIX=$2; shift 2;; --service) SERVICE=$2; shift 2;;
    --binary-url) BINARY_URL=$2; shift 2;; --uninstall) UNINSTALL=1; shift;;
    *) printf 'Unknown option: %s\n' "$1" >&2; exit 2;;
  esac
done
[ "$(id -u)" -eq 0 ] || { echo '请以 root 执行安装或卸载。' >&2; exit 1; }
case "$SERVICE" in ''|*[!a-zA-Z0-9_-]*) echo '服务名称无效。' >&2; exit 1;; esac
case "$PREFIX" in /opt/*) ;; *) echo '安装目录必须位于 /opt 下。' >&2; exit 1;; esac
case "$PREFIX" in *..*|*" "*|*"'"*|*'"'*|*'`'*|*'$'*|*'\'*) echo '安装目录无效。' >&2; exit 1;; esac
command -v systemctl >/dev/null || { echo '此安装器需要 systemd。其他平台可直接运行 Agent 二进制。' >&2; exit 1; }
ENVFILE=$PREFIX/$SERVICE.env
if [ "$UNINSTALL" -eq 1 ]; then
  systemctl stop "$SERVICE" 2>/dev/null || true
  systemctl disable "$SERVICE" 2>/dev/null || true
  rm -f "/etc/systemd/system/$SERVICE.service" "$PREFIX/monitor-agent" "$ENVFILE"
  systemctl daemon-reload
  echo 'Agent 已卸载，其他服务和 Hub 数据保留。'
  exit 0
fi
[ -n "$SERVER" ] && [ -n "$TOKEN" ] || { echo '需要 --server 和 --token。' >&2; exit 2; }
case "$SERVER" in https://*) ;; http://127.0.0.1:*|http://localhost:*) ;; *) echo '远程 Hub 必须使用 HTTPS。' >&2; exit 1;; esac
case "$SERVER" in *"'"*|*'"'*|*'`'*|*'$'*|*'\'*|*" "*) echo 'Hub URL 包含不支持的字符。' >&2; exit 1;; esac
case "$TOKEN" in ''|*[!a-fA-F0-9]*) echo 'Token 格式无效。' >&2; exit 1;; esac
[ "${#TOKEN}" -eq 64 ] || { echo 'Token 需要 64 位十六进制字符。' >&2; exit 1; }
case "$INTERVAL" in ''|*[!0-9]*) echo '上报间隔无效。' >&2; exit 1;; esac
[ "$INTERVAL" -ge 1 ] && [ "$INTERVAL" -le 60 ] || { echo '上报间隔需要 1–60 秒。' >&2; exit 1; }
case "$IFACE" in *[!a-zA-Z0-9_,.-]*) echo '网卡列表格式无效。' >&2; exit 1;; esac
case "$(uname -m)" in x86_64) ARCH=amd64;; aarch64|arm64) ARCH=arm64;; *) echo '此架构尚不支持。' >&2; exit 1;; esac
command -v curl >/dev/null || { echo '请先安装 curl。' >&2; exit 1; }
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
URL=${BINARY_URL:-${SERVER%/}/agent/$ARCH}
curl -fSL --retry 3 --connect-timeout 10 "$URL" -o "$STAGE/monitor-agent"
chmod 755 "$STAGE/monitor-agent"
"$STAGE/monitor-agent" --version
id "$SERVICE" >/dev/null 2>&1 || useradd --system --user-group --no-create-home --shell /usr/sbin/nologin "$SERVICE"
mkdir -p "$PREFIX"
install -m 755 "$STAGE/monitor-agent" "$PREFIX/monitor-agent"
umask 077
printf 'MONITOR_SERVER=%s\nMONITOR_TOKEN=%s\nMONITOR_INTERVAL=%s\nMONITOR_IFACE=%s\n' "$SERVER" "$TOKEN" "$INTERVAL" "$IFACE" > "$ENVFILE"
chmod 600 "$ENVFILE"
cat > "/etc/systemd/system/$SERVICE.service" <<EOF
[Unit]
Description=Monitor read-only agent
After=network-online.target
Wants=network-online.target
[Service]
Type=simple
User=$SERVICE
Group=$SERVICE
EnvironmentFile=$ENVFILE
Environment=RUST_LOG=info
ExecStart=$PREFIX/monitor-agent
Restart=always
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
RestrictAddressFamilies=AF_INET AF_INET6 AF_NETLINK
CapabilityBoundingSet=
MemoryMax=96M
UMask=0077
[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable "$SERVICE"
systemctl restart "$SERVICE"
systemctl is-active --quiet "$SERVICE"
echo 'Agent 已启动。查看日志：journalctl -u '"$SERVICE"' -f'
