# Déploiement de poe2-server

`deploy.sh [utilisateur@hôte]` compile le serveur en binaire statique (musl), l'installe dans
`/opt/poe2-tracker/`, avec le service systemd, la sauvegarde quotidienne et le site Caddy, puis
redémarre et vérifie `https://poe2-tracker.cxxi.io/health`. Relançable à chaque version.

Cible musl à ajouter une fois : `rustup target add x86_64-unknown-linux-musl`.

## Première installation sur une nouvelle machine

Debian (ou tout Linux avec systemd), MariaDB et Caddy installés, le DNS de
`poe2-tracker.cxxi.io` pointant dessus.

1. Utilisateur et dossiers :
   `sudo useradd --system --home /opt/poe2-tracker --shell /usr/sbin/nologin poe2`,
   `sudo install -d -m 755 /opt/poe2-tracker`, `sudo install -d -g poe2 -m 750 /etc/poe2-tracker`.
2. Base : `CREATE DATABASE poe2_tracker CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;`,
   un utilisateur `poe2`@`localhost` avec un mot de passe aléatoire et `GRANT ALL ON
   poe2_tracker.*`. Les tables sont créées par le serveur à son démarrage (migrations).
3. `/etc/poe2-tracker/env` (root:poe2, 640) :
   ```
   DATABASE_URL=mysql://poe2:<mot de passe>@localhost/poe2_tracker
   LISTEN=127.0.0.1:8787
   RUST_LOG=info
   ```
4. Caddy : si son Caddyfile appartient à un autre projet, ne pas y toucher. Créer
   `/etc/caddy/host.caddyfile` (`import /etc/caddy/Caddyfile` puis
   `import /etc/caddy/sites.d/*.caddy`) et un drop-in `/etc/systemd/system/caddy.service.d/host.conf`
   qui remplace `ExecStart` et `ExecReload` par les mêmes commandes avec
   `--config /etc/caddy/host.caddyfile --adapter caddyfile`.
5. `deploy.sh debian@<hôte>`.

## Sauvegardes

`poe2-tracker-backup.timer` : chaque nuit vers 4 h 30, un dump compressé dans
`/var/backups/poe2-tracker/`, les 3 derniers gardés. Restauration :
`gunzip -c <fichier>.sql.gz | sudo mariadb poe2_tracker`.
