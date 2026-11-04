#!/bin/sh
# Sauvegarde quotidienne de la base poe2_tracker : un dump compressé par jour, les 3 derniers
# gardés. Lancé par poe2-tracker-backup.timer, en root (accès MariaDB par socket Unix).
set -eu
dir=/var/backups/poe2-tracker
keep=3
mkdir -p "$dir"
chmod 700 "$dir"
file="$dir/poe2_tracker-$(date +%Y%m%d-%H%M%S).sql.gz"
mariadb-dump --single-transaction --routines poe2_tracker | gzip -9 > "$file.tmp"
mv "$file.tmp" "$file"
# Rotation : seuls les $keep plus récents restent.
ls -1t "$dir"/poe2_tracker-*.sql.gz | tail -n +$((keep + 1)) | xargs -r rm -f
