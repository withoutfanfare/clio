"""CLI entrypoint for the shared memory store."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .config import get_database_path
from .models import LinkInput, MemoryInput, SearchInput
from .render import render_memory_markdown, render_recall_markdown
from .store import MemoryStore


def build_parser() -> argparse.ArgumentParser:
    """Create the CLI parser."""
    parser = argparse.ArgumentParser(prog="ai-memory")
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("init")

    remember = subparsers.add_parser("remember")
    remember.add_argument("--namespace", default="global")
    remember.add_argument("--kind", default="note")
    remember.add_argument("--title")
    remember.add_argument("--summary")
    remember.add_argument("--content", required=True)
    remember.add_argument("--tags", nargs="*", default=[])
    remember.add_argument("--source")
    remember.add_argument("--source-ref")
    remember.add_argument("--confidence", type=float)
    remember.add_argument("--importance", type=int, default=3)
    remember.add_argument("--metadata", default="{}")
    remember.add_argument("--upsert", action="store_true")

    recall = subparsers.add_parser("recall")
    recall.add_argument("--query")
    recall.add_argument("--namespace")
    recall.add_argument("--kind")
    recall.add_argument("--tags", nargs="*", default=[])
    recall.add_argument("--match-any-tags", action="store_true")
    recall.add_argument("--include-archived", action="store_true")
    recall.add_argument("--limit", type=int, default=10)
    recall.add_argument("--offset", type=int, default=0)
    recall.add_argument("--json", action="store_true")

    show = subparsers.add_parser("show")
    show.add_argument("memory_id")
    show.add_argument("--json", action="store_true")

    archive = subparsers.add_parser("archive")
    archive.add_argument("memory_id")

    link = subparsers.add_parser("link")
    link.add_argument("from_memory_id")
    link.add_argument("to_memory_id")
    link.add_argument("--relationship", default="relates_to")
    link.add_argument("--metadata", default="{}")

    export = subparsers.add_parser("export")
    export.add_argument("--output", required=True)

    schema = subparsers.add_parser("schema")
    schema.add_argument("--json", action="store_true")

    return parser


def print_payload(payload: object, as_json: bool) -> None:
    """Print a result in the requested format."""
    if as_json:
        print(json.dumps(payload, indent=2, sort_keys=True))
    else:
        print(payload)


def main() -> None:
    """Run the CLI."""
    parser = build_parser()
    args = parser.parse_args()
    store = MemoryStore(get_database_path())

    try:
        if args.command == "init":
            print(json.dumps(store.initialize(), indent=2, sort_keys=True))
            return

        if args.command == "remember":
            record = store.remember(
                MemoryInput(
                    namespace=args.namespace,
                    kind=args.kind,
                    title=args.title,
                    summary=args.summary,
                    content=args.content,
                    tags=args.tags,
                    source=args.source,
                    source_ref=args.source_ref,
                    confidence=args.confidence,
                    importance=args.importance,
                    metadata=json.loads(args.metadata),
                    upsert=args.upsert,
                )
            )
            print(render_memory_markdown(record))
            return

        if args.command == "recall":
            result = store.search(
                SearchInput(
                    query=args.query,
                    namespace=args.namespace,
                    kind=args.kind,
                    tags=args.tags,
                    match_all_tags=not args.match_any_tags,
                    include_archived=args.include_archived,
                    limit=args.limit,
                    offset=args.offset,
                    response_format="json" if args.json else "markdown",
                )
            )
            if args.json:
                print(result.model_dump_json(indent=2))
            else:
                print(render_recall_markdown(result))
            return

        if args.command == "show":
            record = store.get(args.memory_id)
            if args.json:
                print(record.model_dump_json(indent=2))
            else:
                print(render_memory_markdown(record))
            return

        if args.command == "archive":
            record = store.archive(args.memory_id)
            print(render_memory_markdown(record))
            return

        if args.command == "link":
            payload = store.link(
                LinkInput(
                    from_memory_id=args.from_memory_id,
                    to_memory_id=args.to_memory_id,
                    relationship=args.relationship,
                    metadata=json.loads(args.metadata),
                )
            )
            print(json.dumps(payload, indent=2, sort_keys=True))
            return

        if args.command == "export":
            result = store.export_jsonl(Path(args.output).expanduser())
            print(json.dumps(result, indent=2, sort_keys=True))
            return

        if args.command == "schema":
            snapshot = store.schema_snapshot()
            print_payload(snapshot, as_json=True if args.json else True)
            return
    except Exception as exc:  # noqa: BLE001
        print(f"Error: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc


if __name__ == "__main__":
    main()
