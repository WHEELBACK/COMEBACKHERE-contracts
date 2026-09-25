#!/usr/bin/env python3
from pathlib import Path
import re
import sys

source_events = set()
for path in Path("contracts").rglob("src/**/*.rs"):
    text = path.read_text()
    source_events.update(re.findall(r'Symbol::new\([^,]+,\s*"([^"]+)"\)', text))

documented_events = set()
for line in Path("docs/event-schema.md").read_text().splitlines():
    parts = [part.strip() for part in line.split("|")]
    if len(parts) >= 5 and parts[1].startswith("`") and parts[2].startswith("`("):
        documented_events.add(parts[1].strip("`"))

missing = sorted(source_events - documented_events)
stale = sorted(documented_events - source_events)

if missing or stale:
    if missing:
        print("Events emitted in source but missing from docs/event-schema.md:", file=sys.stderr)
        for event in missing:
            print(f"  - {event}", file=sys.stderr)
    if stale:
        print("Events documented but not found in source:", file=sys.stderr)
        for event in stale:
            print(f"  - {event}", file=sys.stderr)
    sys.exit(1)

print(f"event schema matches source ({len(source_events)} events)")
