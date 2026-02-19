#!/usr/bin/env python3
import json
import sys
from pathlib import Path


def flatten_keys(obj, prefix=""):
    if isinstance(obj, dict):
        keys = set()
        for k, v in obj.items():
            next_prefix = f"{prefix}.{k}" if prefix else k
            keys |= flatten_keys(v, next_prefix)
        return keys
    return {prefix}


def main() -> int:
    path = Path("ux/i18n_keys.json")
    if not path.exists():
        print("ERROR: ux/i18n_keys.json not found")
        return 1

    data = json.loads(path.read_text(encoding="utf-8"))

    missing_langs = [lang for lang in ("pt", "en") if lang not in data]
    if missing_langs:
        print(f"ERROR: missing language blocks: {', '.join(missing_langs)}")
        return 1

    pt_keys = flatten_keys(data["pt"])
    en_keys = flatten_keys(data["en"])

    only_pt = sorted(pt_keys - en_keys)
    only_en = sorted(en_keys - pt_keys)

    if only_pt or only_en:
        print("ERROR: i18n key mismatch")
        if only_pt:
            print("  keys only in pt:")
            for k in only_pt:
                print(f"    - {k}")
        if only_en:
            print("  keys only in en:")
            for k in only_en:
                print(f"    - {k}")
        return 1

    print(f"OK: i18n keys matched ({len(pt_keys)} keys)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
