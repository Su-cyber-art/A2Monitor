#!/bin/sh
set -eu
PACKAGE= PREFIX=/opt/monitor-hub SERVICE=monitor-hub LISTEN=127.0.0.1:28080 UNINSTALL=0
while [ "$#" -gt 0 ]; do
  case "$1" in --package) PACKAGE=$2;shift 2;; --prefix) PREFIX=$2;shift 2;; --service) SERVICE=$2;shift 2;; --listen) LISTEN=$2;shift 2;; --uninstall) UNINSTALL=1;shift;; *) echo '参数无效。' >&2;exit 2;; esac
done
[ "$(id -u)" -eq 0 ] || { echo '请以 root 执行。' >&2;exit 1; }
case "$SERVICE" in ''|*[!a-zA-Z0-9_-]*) exit 2;; esac
case "$PREFIX" in /opt/*) ;; *) exit 2;; esac
case "$PREFIX" in *..*|*" "*|*"'"*|*'"'*|*'$'*|*'`'*|*'\'*) exit 2;; esac
command -v systemctl >/dev/null || { echo '需要 systemd。' >&2;exit 1; }
if [ "$UNINSTALL" -eq 1 ]; then
  systemctl stop "$SERVICE" 2>/dev/null || true
  systemctl disable "$SERVICE" 2>/dev/null || true
  rm -f "/etc/systemd/system/$SERVICE.service" "$PREFIX/bin/monitor-hub" "$PREFIX/bin/monitor-agent"
  systemctl daemon-reload
  echo "Hub 已卸载，数据库仍保留在 $PREFIX/data。"
  exit 0
fi
[ -f "$PACKAGE" ] || { echo '需要 --package 指定构建生成的 Monitor 发布包。' >&2;exit 2; }
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
tar -xzf "$PACKAGE" -C "$STAGE"
[ -f "$STAGE/bin/monitor-hub" ] && [ -f "$STAGE/web/dist/index.html" ] || { echo '发布包缺少程序或前端。' >&2;exit 1; }
"$STAGE/bin/monitor-hub" --version
id "$SERVICE" >/dev/null 2>&1 || useradd --system --user-group --no-create-home --shell /usr/sbin/nologin "$SERVICE"
mkdir -p "$PREFIX/bin" "$PREFIX/web" "$PREFIX/data"
install -m 755 "$STAGE/bin/monitor-hub" "$PREFIX/bin/monitor-hub"
install -m 755 "$STAGE/bin/monitor-agent" "$PREFIX/bin/monitor-agent"
cp -R "$STAGE/web/dist" "$PREFIX/web/"
if [ -d "$STAGE/web/themes" ]; then cp -R "$STAGE/web/themes" "$PREFIX/web/"; fi
chown -R "$SERVICE:$SERVICE" "$PREFIX/data"
chmod 700 "$PREFIX/data"
cat > "/etc/systemd/system/$SERVICE.service" <<EOF
[Unit]
Description=Monitor Hub
After=network-online.target
Wants=network-online.target
[Service]
Type=simple
User=$SERVICE
Group=$SERVICE
WorkingDirectory=$PREFIX
ExecStart=$PREFIX/bin/monitor-hub --listen $LISTEN --data $PREFIX/data --frontend $PREFIX/web/dist
Environment=RUST_LOG=info
Restart=always
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=$PREFIX/data
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
RestrictSUIDSGID=true
CapabilityBoundingSet=
RestrictAddressFamilies=AF_INET AF_INET6
MemoryMax=256M
UMask=0077
[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable "$SERVICE"
systemctl restart "$SERVICE"
echo "Hub 已启动，监听 $LISTEN。初始密码：journalctl -u $SERVICE --no-pager"
