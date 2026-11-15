#!/usr/bin/env python3
"""Fail when tracked files look like private data, or README uses private-domain words."""

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 100_000
ALLOW_LARGE = {
    "Cargo.lock",
    "web/package-lock.json",
    "web/src-tauri/Cargo.lock",
}
FORBIDDEN_SUFFIXES = {
    ".oml",
    ".xlsx",
    ".xls",
    ".xlsm",
    ".pdf",
    ".doc",
    ".docx",
    ".ppt",
    ".pptx",
    ".csv",
    ".tsv",
    ".sqlite",
    ".sqlite3",
    ".db",
    ".accdb",
    ".zip",
    ".7z",
    ".rar",
    ".pst",
    ".msg",
}
FORBIDDEN_DIRS = {".hub", "bench-data"}
README_WORDS = (
    "お客様",
    "顧客",
    "注文",
    "配送",
    "請求",
    "商品",
    "機密",
    "実文書",
    "システムA",
    "仕様書",
    "社内",
    "業務",
    "Specifications",
    "SystemA",
)


def tracked_files() -> list[str]:
    output = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT,
    )
    return [item.decode() for item in output.split(b"\0") if item]


def main() -> int:
    errors: list[str] = []
    files = tracked_files()
    for rel in files:
        path = Path(rel)
        if set(path.parts) & FORBIDDEN_DIRS:
            errors.append(f"索引や測定データを追跡しています: {rel}")
        if path.suffix.lower() in FORBIDDEN_SUFFIXES:
            errors.append(f"データファイルを追跡しています: {rel}")
        full = ROOT / rel
        if full.is_file() and full.stat().st_size > MAX_BYTES and rel not in ALLOW_LARGE:
            errors.append(f"100KBを超えるファイルです: {rel}")
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    for word in README_WORDS:
        if word in readme:
            errors.append(f"READMEに私的な領域の語があります: {word}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"追跡ファイル {len(files)} 件を確認しました")
    return 0


if __name__ == "__main__":
    sys.exit(main())
