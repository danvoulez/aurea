#!/usr/bin/env bash
# tools/smoke_repair.sh — Smoke test B: Auto-reparo N=2
# Demonstrates: incomplete payload → repair_request → patch → OK → then fail after N=2
# Usage: bash tools/smoke_repair.sh [BASE_URL]
set -euo pipefail

BASE_URL="${1:-http://localhost:8080}"
ARTIFACTS="artifacts/smoke"
mkdir -p "$ARTIFACTS"
LOG="$ARTIFACTS/repair.log"

log() { echo "[repair] $*" | tee -a "$LOG"; }

log "=== Smoke B: Auto-reparo N=2 ==="

# Step 1 — Send incomplete payload (missing height, bitrate)
log "Step 1: plan_preview with incomplete payload (attempt=0)"
PREVIEW1=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d '{
      "intent": {
        "schema_id": "vcx.batch_transcode",
        "v": "1",
        "topic": "vcx:commit",
        "payload": {"codec": "av1", "width": 640}
      },
      "repair_attempt": 0
    }' \
    "$BASE_URL/v1/oc/plan_preview")
echo "$PREVIEW1" | tee "$ARTIFACTS/repair_step1_preview1.json"

REPAIR=$(echo "$PREVIEW1" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(json.dumps(d.get("repair_request",{})))')
MISSING=$(echo "$PREVIEW1" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("repair_request",{}).get("missing",[]))' 2>/dev/null || echo "[]")
log "repair_request: missing=$MISSING"

if [ "$REPAIR" = "{}" ] || [ "$REPAIR" = "null" ]; then
    log "FAIL: expected repair_request in response, got none"
    exit 1
fi

# Step 2 — Provide missing fields (repair attempt 1)
log "Step 2: plan_preview with repaired payload (attempt=1)"
PREVIEW2=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d '{
      "intent": {
        "schema_id": "vcx.batch_transcode",
        "v": "1",
        "topic": "vcx:commit",
        "payload": {"codec": "av1", "width": 640, "height": 360, "bitrate": 600}
      },
      "repair_attempt": 1
    }' \
    "$BASE_URL/v1/oc/plan_preview")
echo "$PREVIEW2" | tee "$ARTIFACTS/repair_step2_preview2.json"

PLAN_HASH=$(echo "$PREVIEW2" | python3 -c 'import sys,json; print(json.load(sys.stdin)["plan_hash"])')
REPAIR2=$(echo "$PREVIEW2" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("repair_request"))' 2>/dev/null || echo "None")
log "plan_hash=$PLAN_HASH repair2=$REPAIR2"

if [ -z "$PLAN_HASH" ] || [ "$PLAN_HASH" = "null" ] || [ "$PLAN_HASH" = "" ]; then
    log "FAIL: expected valid plan_hash after repair, got '$PLAN_HASH'"
    exit 1
fi
log "Repair attempt 1 succeeded, plan_hash=$PLAN_HASH"

# Step 3 — Commit the repaired plan
log "Step 3: commit repaired plan"
COMMIT=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d "{\"plan_hash\":\"$PLAN_HASH\",\"confirm_phrase\":\"Conferi e confirmo o plano.\"}" \
    "$BASE_URL/v1/oc/commit")
echo "$COMMIT" | tee "$ARTIFACTS/repair_step3_commit.json"
log "Commit: $(echo "$COMMIT" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d["status"])')"

# Step 4 — Exhaust repair budget (attempt=2 with still-missing fields → SCHEMA_INVALID)
log "Step 4: plan_preview with missing fields and repair_attempt=2 → expect SCHEMA_INVALID"
HTTP_CODE=$(curl -sS -o "$ARTIFACTS/repair_step4_exhausted.json" -w "%{http_code}" \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d '{
      "intent": {
        "schema_id": "vcx.batch_transcode",
        "v": "1",
        "topic": "vcx:commit",
        "payload": {"codec": "av1"}
      },
      "repair_attempt": 2
    }' \
    "$BASE_URL/v1/oc/plan_preview")
cat "$ARTIFACTS/repair_step4_exhausted.json"
log "HTTP code for exhausted repair: $HTTP_CODE"

if [ "$HTTP_CODE" != "422" ]; then
    log "FAIL: expected 422 SCHEMA_INVALID after repair budget exhausted, got $HTTP_CODE"
    exit 1
fi

ERROR_CODE=$(cat "$ARTIFACTS/repair_step4_exhausted.json" | python3 -c 'import sys,json; print(json.load(sys.stdin)["error"]["code"])' 2>/dev/null || echo "")
log "Error code: $ERROR_CODE"

if [ "$ERROR_CODE" != "SCHEMA_INVALID" ]; then
    log "FAIL: expected error code SCHEMA_INVALID, got $ERROR_CODE"
    exit 1
fi

log "=== PASS: Auto-reparo N=2 confirmado ==="
