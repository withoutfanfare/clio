#!/usr/bin/env python3
"""Smoke-test app contracts through real CLI/MCP adapters using disposable data.

Build first: cargo build --locked -p clio-cli -p clio-mcp --no-default-features
Run: python3 scripts/verify-app-contracts.py
No live database, remote route, model provider or user settings are used.
"""

import argparse
import json
import os
from pathlib import Path
import selectors
import sqlite3
import subprocess
import tempfile
import time
import uuid


TIMEOUT = 15
ROOT = Path(__file__).resolve().parents[1]


def check(condition, message):
    if not condition:
        raise AssertionError(message)


def ids(result):
    return [item["id"] for item in result["items"]]


class Mcp:
    """Sequential JSON-lines requests, with a deadline and bounded shutdown."""

    def __init__(self, binary, directory, env):
        self.stderr = tempfile.TemporaryFile()
        self.process = subprocess.Popen(
            [str(binary)], cwd=directory, env=env,
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr,
        )
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.buffer = b""
        self.request_id = 0

    def __enter__(self):
        try:
            self.request("initialize", {
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "clio-app-contract-smoke", "version": "1"},
            })
            self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
            return self
        except BaseException:
            self.close()
            raise

    def send(self, message):
        self.process.stdin.write(json.dumps(message).encode() + b"\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.request_id += 1
        request_id = self.request_id
        self.send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + TIMEOUT
        while True:
            while b"\n" in self.buffer:
                line, self.buffer = self.buffer.split(b"\n", 1)
                response = json.loads(line)
                if response.get("id") == request_id:
                    check("error" not in response, f"MCP {method}: {response}")
                    return response["result"]
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not self.selector.select(remaining):
                raise TimeoutError(f"MCP {method} did not reply within {TIMEOUT}s")
            chunk = os.read(self.process.stdout.fileno(), 65536)
            if not chunk:
                self.stderr.seek(0)
                raise RuntimeError(f"MCP exited: {self.stderr.read().decode(errors='replace')}")
            self.buffer += chunk

    def tool(self, name, **arguments):
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        check(not result.get("isError"), f"MCP {name}: {result}")
        return json.loads("\n".join(
            item["text"] for item in result["content"] if item["type"] == "text"
        ))

    def close(self):
        try:
            self.process.stdin.close()
        except BrokenPipeError:
            pass
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)
        self.selector.close()
        self.process.stdout.close()
        self.stderr.close()
        check(self.process.poll() is not None, "MCP process must stop")

    def __exit__(self, *_):
        self.close()


def run(cli_binary, mcp_binary):
    passed = 0

    def passed_check(name):
        nonlocal passed
        passed += 1
        print(f"PASS {name}", flush=True)

    with tempfile.TemporaryDirectory(prefix="clio-app-contracts-") as directory:
        directory = Path(directory)
        db = directory / "memory.db"
        env = {key: value for key, value in os.environ.items()
               if not key.startswith("CLIO_") and key not in ("OPENAI_API_KEY", "OPENAI_API_KEY_CLIO")}
        env["CLIO_DB_PATH"] = str(db)
        settings = {
            "auto_embed": False, "embeddings": {"provider": "disabled"},
            "capture": {"enabled": False}, "auto_title": {"enabled": False},
            "context": {"auto_detect": False}, "remote": None,
        }
        (directory / "clio-settings.json").write_text(json.dumps(settings))

        def cli(*arguments):
            result = subprocess.run(
                [str(cli_binary), "--local", "--db-path", str(db), "--json", *arguments],
                cwd=directory, env=env, capture_output=True, text=True, timeout=TIMEOUT,
            )
            check(result.returncode == 0, f"CLI {' '.join(arguments)}: {result.stderr}")
            return json.loads(result.stdout) if result.stdout.strip() else None

        cli("init")

        def remember(namespace, title, kind="note", tag="alpha-tag"):
            memory = cli("remember", "--namespace", namespace, "--title", title,
                         "--content", "Contract orchard " + title, "--kind", kind, "--tags", tag)
            check(str(uuid.UUID(memory["id"])) == memory["id"], "Full memory UUID required")
            return memory["id"]

        alpha, beta = "smoke:alpha", "smoke:beta"
        first = remember(alpha, "First evidence")
        second = remember(alpha, "Second evidence")
        archived = remember(alpha, "Archived evidence")
        remember(beta, "Other workspace", "decision", "beta-tag")
        cli("archive", archived)

        # Seed queue entries directly only in this migrated, disposable database.
        # No public queue command exists without invoking the capture provider.
        review_ids = [str(uuid.uuid4()), str(uuid.uuid4())]
        with sqlite3.connect(db) as conn:
            conn.executemany(
                "INSERT INTO review_queue (id, content, suggested_namespace, suggested_title, "
                "created_at, source_route) VALUES (?, ?, ?, ?, ?, 'contract-smoke')",
                [(item, f"Review content {index}", alpha, f"Suggestion {index}",
                  f"2026-01-0{index + 1}T00:00:00Z") for index, item in enumerate(review_ids)],
            )

        with Mcp(mcp_binary, directory, env) as mcp:
            def recall(**arguments):
                return mcp.tool("memory_recall", response_format="json", **arguments)

            for result in (cli("recall", "--namespace", alpha), recall(namespace=alpha)):
                check(set(ids(result)) == {first, second} and result["total"] == 2,
                      "Default recall must exclude archived records")
                check(result["archived_only"] is False, "Active recall marker must be false")
            for result in (
                cli("recall", "--namespace", alpha, "--archived-only", "--include-archived"),
                recall(namespace=alpha, archived_only=True, include_archived=True),
            ):
                check(ids(result) == [archived] and result["archived_only"] is True,
                      "Archive-only takes precedence and confirms filtering")
            for result in (cli("recall", "--namespace", beta, "--archived-only"),
                           recall(namespace=beta, archived_only=True)):
                check(result["items"] == [] and result["archived_only"] is True,
                      "Empty Archive must still confirm backend support")
            passed_check("active/archive-only filters and capability marker across CLI/MCP")

            cli_page = cli("recent", "--namespace", alpha, "--limit", "1", "--offset", "1")
            mcp_page = mcp.tool("memory_recent", namespace=alpha, limit=1, offset=1, response_format="json")
            check(ids(cli_page) == ids(mcp_page) and len(ids(cli_page)) == 1,
                  "Recent adapters must agree on the second page")
            for result in (cli_page, mcp_page):
                check(result["total"] == 2 and result["offset"] == 1, "Recent pagination metadata")
            for result in (cli("recent", "--namespace", alpha, "--offset", "99"),
                           mcp.tool("memory_recent", namespace=alpha, offset=99, response_format="json")):
                check(result["items"] == [] and result["total"] == 2, "Past-end pages retain total")
            passed_check("recent offset and past-end totals across CLI/MCP")

            for result in (cli("recall", "--namespace", alpha, "--query", "orchard", "--archived-only"),
                           recall(namespace=alpha, query="orchard", archived_only=True)):
                check(ids(result) == [archived] and result["archived_only"] is True,
                      "FTS must honour archive-only")
            passed_check("FTS archived-only contract across CLI/MCP")

            for result in (cli("stats", "--namespace", alpha),
                           mcp.tool("memory_stats", namespace=alpha, response_format="json")):
                check(result["namespace"] == alpha and result["total_memories"] == 3,
                      "Stats scope echo and total")
                check(result["active_memories"] == 2 and result["archived_memories"] == 1,
                      "Stats active/archive denominators")
                check(result["by_namespace"] == [[alpha, 3]] and result["by_kind"] == [["note", 3]],
                      "Stats breakdowns must exclude other workspaces")
                check(result["top_tags"] == [["alpha-tag", 3]], "Tag counts must be scoped")
            passed_check("scoped statistics echo, counts and breakdowns across CLI/MCP")

            for memory_id in (first, second, archived):
                mcp.tool("memory_action", action="add", memory_id=memory_id,
                         namespace=alpha, trigger="project-session")
            with sqlite3.connect(db) as conn:
                conn.execute("UPDATE memories SET valid_until='2000-01-01T00:00:00Z' WHERE id=?", (second,))
                before = conn.execute("SELECT SUM(access_count) FROM memories").fetchone()[0]
            overview = mcp.tool("memory_action", action="overview", namespace=alpha)
            check(overview["memory_titles"] == {first: "First evidence"},
                  "Attention titles must exclude archived and expired evidence")
            check(overview["review_pending"] == 2, "Attention must report unresolved queue depth")
            with sqlite3.connect(db) as conn:
                check(conn.execute("SELECT SUM(access_count) FROM memories").fetchone()[0] == before,
                      "Attention overview must not inflate access tracking")
            passed_check("MCP attention titles respect eligibility and leave access tracking unchanged")

            legacy = mcp.tool("memory_inbox", action="list", response_format="json")
            scoped = mcp.tool("memory_inbox", action="list", response_format="json", include_status_scope=True)
            check(isinstance(legacy, list) and [item["id"] for item in legacy] == review_ids,
                  "Legacy inbox remains an oldest-first array")
            check(scoped["includes_edited"] is True and scoped["items"] == legacy,
                  "Opt-in inbox envelope confirms unresolved status coverage")
            check(cli("inbox", "list") == legacy, "CLI/MCP inbox lists must agree")
            passed_check("inbox legacy array and opt-in status envelope")

            edited = mcp.tool("memory_inbox", action="edit", review_id=review_ids[0],
                              title="Reviewed suggestion", namespace=beta, importance=4)
            check(edited["status"] == "edited", "Edit must remain unresolved")
            pending = cli("inbox", "list")
            check([item["id"] for item in pending] == review_ids and pending[0]["status"] == "edited",
                  "Edited review must remain visible in CLI")
            pending_mcp = mcp.tool("memory_inbox", action="list", response_format="json", include_status_scope=True)
            check(pending_mcp["items"] == pending, "Edited review must remain visible in MCP")
            check(mcp.tool("memory_action", action="overview", namespace=alpha)["review_pending"] == 2,
                  "Edited review remains included in attention depth")
            passed_check("edited suggestions stay unresolved and visible across adapters")

            approved = mcp.tool("memory_inbox", action="approve", review_id=review_ids[0])
            check(approved["title"] == "Reviewed suggestion" and approved["namespace"] == beta
                  and approved["importance"] == 4, "Approval must use edited suggestions")
            check(cli("show", approved["id"])["content"] == "Review content 0",
                  "Approved memory must be readable through CLI")
            rejected = cli("inbox", "reject", review_ids[1])
            check(rejected["status"] == "rejected", "Rejection must be confirmed")
            check(mcp.tool("memory_inbox", action="list", response_format="json") == [],
                  "Confirmed approval/rejection must leave no unresolved reviews")
            with sqlite3.connect(db) as conn:
                check(conn.execute("SELECT COUNT(*) FROM memories WHERE content='Review content 1'").fetchone()[0] == 0,
                      "Rejection must not create a memory")
            passed_check("confirmed approval stores edited fields; rejection creates no memory")

        check(mcp.process.poll() is not None, "MCP must stop before temporary data is removed")
    check(not directory.exists(), "Disposable directory must be removed")
    passed_check("MCP stopped and disposable database/settings removed")
    print(f"{passed} contract checks passed; no live settings, database or providers used.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "target" / "debug",
                        help="Directory containing fresh clio and clio-mcp binaries")
    args = parser.parse_args()
    binaries = [(args.bin_dir / name).resolve() for name in ("clio", "clio-mcp")]
    for binary in binaries:
        if not binary.is_file():
            parser.error(f"Missing {binary}; build the adapters first")
    run(*binaries)
