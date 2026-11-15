#!/usr/bin/env bash
# Restore a backup into a NEW database (never overwrites a live one).
#   ADMIN_URL=postgres://admin@host/postgres ./scripts/restore.sh backups/chimppos_X.dump chimp_pos_restored
# Then inspect it, and switch DATABASE_URL to it (or dump/restore into production) when you are sure.
set -euo pipefail
FILE="${1:?usage: restore.sh <dump file> <new database name>}"
NEWDB="${2:?usage: restore.sh <dump file> <new database name>}"
: "${ADMIN_URL:?ADMIN_URL must point to the 'postgres' maintenance database with CREATEDB rights}"
[[ "$NEWDB" =~ ^[a-zA-Z0-9_]+$ ]] || { echo "database name may only contain letters, digits, underscore"; exit 2; }
psql "$ADMIN_URL" -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$NEWDB\""
BASE="${ADMIN_URL%/*}"
pg_restore --no-owner --exit-on-error --dbname="$BASE/$NEWDB" "$FILE"
# A restored database has NO planner statistics: without this, some queries are 100-1000x slower.
psql "$BASE/$NEWDB" -qc "ANALYZE"
echo "Restored into database '$NEWDB'. Row counts:"
psql "$BASE/$NEWDB" -At -c "select 'companies', count(*) from companies union all select 'users', count(*) from users union all select 'sales', count(*) from sales union all select 'products', count(*) from products"
