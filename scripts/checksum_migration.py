#!/usr/bin/env python3
"""Print blake3 hex digest of a file (for sqlx migration checksum fix)."""
import sys
try:
    import blake3
except ImportError:
    sys.exit("pip install blake3")
path = sys.argv[1] if len(sys.argv) > 1 else "migrations/20240101000002_gameresult_slot.sql"
with open(path, "rb") as f:
    print(blake3.blake3(f.read()).hexdigest())
