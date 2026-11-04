#!/bin/sh
# Déploie poe2-server : binaire statique (musl, tourne sur n'importe quel Linux x86_64),
# service systemd, sauvegarde quotidienne et site Caddy. Relançable : ne recrée ni la base ni
# /etc/poe2-tracker/env s'ils existent (première installation : voir README.md à côté).
#   server/deploy/deploy.sh [utilisateur@hôte]
set -eu
host=${1:-debian@51.178.31.19}
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cargo=${CARGO:-$HOME/.cargo/bin/cargo}

"$cargo" build --release -p poe2-server --target x86_64-unknown-linux-musl --manifest-path "$root/Cargo.toml"
tmp=$(ssh "$host" mktemp -d)
scp -q "$root/target/x86_64-unknown-linux-musl/release/poe2-server" "$here/poe2-tracker.service" "$here/poe2-tracker-backup.sh" \
    "$here/poe2-tracker-backup.service" "$here/poe2-tracker-backup.timer" "$here/poe2-tracker.caddy" "$host:$tmp/"
ssh "$host" "set -eu; cd $tmp
sudo install -m 755 poe2-server poe2-tracker-backup.sh /opt/poe2-tracker/
# Modèles de l'OCR (ocrs), servis à l'app sous /models/ : récupérés une fois, empreinte vérifiée.
sudo install -d -m 755 /opt/poe2-tracker/models
for m in text-detection.onnx:a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe text-recognition.onnx:86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5; do
    f=\${m%%:*}; sum=\${m##*:}
    [ -f /opt/poe2-tracker/models/\$f ] && continue
    curl -fsSL https://ocrs-models.s3-accelerate.amazonaws.com/\$f -o \$f
    echo \"\$sum  \$f\" | sha256sum -c --quiet
    sudo install -m 644 \$f /opt/poe2-tracker/models/
done
sudo install -m 644 poe2-tracker.service poe2-tracker-backup.service poe2-tracker-backup.timer /etc/systemd/system/
sudo install -d -m 755 /etc/caddy/sites.d
sudo install -m 644 poe2-tracker.caddy /etc/caddy/sites.d/
sudo systemctl daemon-reload
sudo systemctl enable --now poe2-tracker-backup.timer
sudo systemctl restart poe2-tracker
sudo caddy validate --config /etc/caddy/host.caddyfile --adapter caddyfile >/dev/null 2>&1 && sudo systemctl reload caddy
rm -rf $tmp"
sleep 2
curl -fsS https://poe2-tracker.cxxi.io/health && echo " — déployé"
