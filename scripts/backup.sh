#!/usr/bin/env bash
# Nightly PostgreSQL backup with rotation and verification.
#   DATABASE_URL=postgres://... BACKUP_DIR=/var/backups/chimppos KEEP_DAYS=14 ./scripts/backup.sh
# Cron (02:30 daily):  30 2 * * * /opt/chimppos/scripts/backup.sh >> /var/log/chimppos-backup.log 2>&1
set -euo pipefail
: "${DATABASE_URL:?DATABASE_URL must be set}"
BACKUP_DIR="${BACKUP_DIR:-./backups}"
KEEP_DAYS="${KEEP_DAYS:-14}"
mkdir -p "$BACKUP_DIR"
chmod 700 "$BACKUP_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
FILE="$BACKUP_DIR/chimppos_${STAMP}.dump"
TMP="$FILE.partial"

pg_dump --format=custom --no-owner --dbname="$DATABASE_URL" --file="$TMP"
# A backup you cannot read back is not a backup: verify the archive's table of contents.
pg_restore --list "$TMP" > /dev/null
mv "$TMP" "$FILE"
chmod 600 "$FILE"
echo "$(date -u +%FT%TZ) backup ok: $FILE ($(du -h "$FILE" | cut -f1))"

find "$BACKUP_DIR" -name 'chimppos_*.dump' -mtime +"$KEEP_DAYS" -print -delete

# Optional off-site copy: set BACKUP_RSYNC_TARGET=user@host:/path  (a backup on the same disk is not enough)
if [ -n "${BACKUP_RSYNC_TARGET:-}" ]; then
  rsync -a "$FILE" "$BACKUP_RSYNC_TARGET/" && echo "copied off-site to $BACKUP_RSYNC_TARGET"
fi
