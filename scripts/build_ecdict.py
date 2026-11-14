#!/usr/bin/env python3
"""Build the bundled ECDICT SQLite resource from a pinned upstream revision."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import sqlite3
import sys
import tempfile
import urllib.request
from pathlib import Path

ECDICT_COMMIT = "ab22ad36302cafc6349a050e2b60894855208055"
ECDICT_URL = (
    "https://raw.githubusercontent.com/skywind3000/ECDICT/"
    f"{ECDICT_COMMIT}/ecdict.csv"
)
EXPECTED_SHA256 = os.environ.get(
    "ENEYE_ECDICT_SHA256",
    "d1ecbdf9ee2b920afb6f430ca6b6bb9f559588cf0dff86e04bd255f9390843cc",
)
SCHEMA_VERSION = 1


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(source: Path) -> None:
    source.parent.mkdir(parents=True, exist_ok=True)
    temporary = source.with_suffix(".download")
    request = urllib.request.Request(
        ECDICT_URL,
        headers={"User-Agent": "Eneye-build/0.1"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        with temporary.open("wb") as target:
            while chunk := response.read(1024 * 1024):
                target.write(chunk)
    temporary.replace(source)


def normalize(word: str) -> str:
    return (
        word.strip()
        .replace("\u2019", "'")
        .casefold()
    )


def build_database(csv_path: Path, destination: Path) -> int:
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        prefix="ecdict-", suffix=".sqlite", dir=destination.parent, delete=False
    ) as handle:
        temporary = Path(handle.name)

    try:
        database = sqlite3.connect(temporary)
        database.executescript(
            """
            PRAGMA journal_mode = OFF;
            PRAGMA synchronous = OFF;
            PRAGMA temp_store = MEMORY;
            CREATE TABLE entries (
                id INTEGER PRIMARY KEY,
                word TEXT NOT NULL,
                word_norm TEXT NOT NULL,
                phonetic TEXT NOT NULL DEFAULT '',
                definition TEXT NOT NULL DEFAULT '',
                translation TEXT NOT NULL DEFAULT '',
                pos TEXT NOT NULL DEFAULT '',
                collins INTEGER,
                oxford INTEGER,
                bnc INTEGER,
                frq INTEGER,
                exchange TEXT NOT NULL DEFAULT '',
                detail TEXT NOT NULL DEFAULT '',
                audio TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            """
        )
        with csv_path.open("r", encoding="utf-8-sig", newline="") as source:
            reader = csv.DictReader(source)
            expected = {
                "word",
                "phonetic",
                "definition",
                "translation",
                "pos",
                "collins",
                "oxford",
                "tag",
                "bnc",
                "frq",
                "exchange",
                "detail",
                "audio",
            }
            if set(reader.fieldnames or []) != expected:
                raise ValueError(
                    f"unexpected ECDICT header: {reader.fieldnames!r}"
                )

            rows = (
                (
                    row["word"],
                    normalize(row["word"]),
                    row["phonetic"],
                    row["definition"],
                    row["translation"],
                    row["pos"],
                    int(row["collins"] or 0),
                    int(row["oxford"] or 0),
                    int(row["bnc"] or 0),
                    int(row["frq"] or 0),
                    row["exchange"],
                    row["detail"],
                    row["audio"],
                )
                for row in reader
                if row["word"].strip()
            )
            database.executemany(
                """
                INSERT INTO entries (
                    word, word_norm, phonetic, definition, translation, pos,
                    collins, oxford, bnc, frq, exchange, detail, audio
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                rows,
            )

        count = database.execute("SELECT count(*) FROM entries").fetchone()[0]
        metadata = {
            "schema_version": str(SCHEMA_VERSION),
            "source": "ECDICT",
            "source_commit": ECDICT_COMMIT,
            "source_sha256": sha256(csv_path),
            "entry_count": str(count),
        }
        database.executemany(
            "INSERT INTO metadata (key, value) VALUES (?, ?)",
            metadata.items(),
        )
        database.execute(
            "CREATE INDEX entries_word_norm ON entries(word_norm, word)"
        )
        database.execute(
            "CREATE INDEX entries_frequency ON entries(frq DESC, bnc DESC)"
        )
        database.commit()
        database.execute("VACUUM")
        database.close()

        temporary.replace(destination)
        manifest = {
            **metadata,
            "sqlite_sha256": sha256(destination),
            "sqlite_bytes": destination.stat().st_size,
        }
        destination.with_suffix(".manifest.json").write_text(
            json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        return count
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--csv",
        type=Path,
        default=Path(
            os.environ.get(
                "ENEYE_ECDICT_CSV",
                str(Path(__file__).resolve().parents[1] / "data" / "ecdict.csv"),
            )
        ),
        help="ECDICT CSV 路径；缺省会下载到仓库内 data/ecdict.csv",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(__file__).resolve().parents[1]
        / "src-tauri"
        / "resources"
        / "ecdic.sqlite",
    )
    args = parser.parse_args()

    if not args.csv.exists():
        print(f"ECDICT source not found; downloading to {args.csv}")
        download(args.csv)

    actual_sha256 = sha256(args.csv)
    if EXPECTED_SHA256 and actual_sha256 != EXPECTED_SHA256:
        raise ValueError(
            f"ECDICT checksum mismatch: {actual_sha256} != {EXPECTED_SHA256}"
        )

    manifest_path = args.output.with_suffix(".manifest.json")
    if args.output.exists() and manifest_path.exists():
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if (
            manifest.get("source_sha256") == actual_sha256
            and manifest.get("schema_version") == str(SCHEMA_VERSION)
        ):
            print(f"Using cached {args.output}")
            return 0

    count = build_database(args.csv, args.output)
    print(
        f"Built {args.output} with {count:,} entries "
        f"({args.output.stat().st_size:,} bytes)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
