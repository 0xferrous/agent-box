#!/usr/bin/env python3
"""Generate gh leaf command operation reports (Markdown + JSON).

Traversal source:
  - gh --help
  - gh <subcommand> --help
  - ... recursively

Outputs in portal directory:
  - portal/gh-leaf-command-read-write-report.md
  - portal/gh-leaf-command-read-write-report.json
"""

import json
import re
import subprocess
from collections import OrderedDict
from pathlib import Path


ENTRY_RE = re.compile(r"^\s{2,}([a-zA-Z0-9][a-zA-Z0-9_-]*)\s*:\s+(.*)$")


def gh_help(path_tokens: list[str]) -> str:
    cmd = ["gh", *path_tokens, "--help"] if path_tokens else ["gh", "--help"]
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.stdout or ""


def parse_subcommands(help_text: str) -> list[tuple[str, str]]:
    entries: list[tuple[str, str]] = []
    in_commands_block = False

    for line in help_text.splitlines():
        stripped = line.strip()

        if re.match(r"^[A-Z0-9 ()-]*COMMANDS$", stripped):
            in_commands_block = True
            continue

        if (
            in_commands_block
            and re.match(r"^[A-Z][A-Z0-9 ()-]*$", stripped)
            and "COMMANDS" not in stripped
        ):
            in_commands_block = False

        if in_commands_block:
            m = ENTRY_RE.match(line)
            if m:
                entries.append((m.group(1), m.group(2).strip()))

    out: list[tuple[str, str]] = []
    seen = set()
    for name, desc in entries:
        if name not in seen:
            seen.add(name)
            out.append((name, desc))
    return out


def classify_operation(full_cmd: str, desc: str) -> str:
    explicit = {
        "api": "Read/Write",
        "browse": "Read",
        "co": "Write",
        "copilot": "Read/Write",
        "completion": "Read",
        "extension exec": "Read/Write",
        "extension browse": "Read/Write",
        "preview prompter": "Read/Write",
        "codespace cp": "Read/Write",
        "codespace ssh": "Read/Write",
        "codespace code": "Read/Write",
        "codespace jupyter": "Read/Write",
        "codespace ports forward": "Read/Write",
        "run watch": "Read",
        "release verify-asset": "Read",
        "release download": "Read",
    }

    if full_cmd in explicit:
        return explicit[full_cmd]

    leaf = full_cmd.split()[-1]

    read_verbs = {
        "list",
        "view",
        "status",
        "checks",
        "diff",
        "download",
        "verify",
        "search",
        "get",
        "token",
        "logs",
        "watch",
        "trusted-root",
        "check",
    }

    write_verbs = {
        "create",
        "delete",
        "edit",
        "refresh",
        "setup-git",
        "switch",
        "stop",
        "clone",
        "rename",
        "close",
        "comment",
        "lock",
        "reopen",
        "unlock",
        "merge",
        "ready",
        "revert",
        "review",
        "update-branch",
        "archive",
        "unarchive",
        "set-default",
        "sync",
        "cancel",
        "rerun",
        "run",
        "enable",
        "disable",
        "set",
        "add",
        "remove",
        "import",
        "upload",
        "fork",
        "transfer",
        "pin",
        "unpin",
        "checkout",
        "link",
        "unlink",
        "mark-template",
        "field-create",
        "field-delete",
        "item-add",
        "item-archive",
        "item-create",
        "item-delete",
        "item-edit",
    }

    if leaf in read_verbs:
        return "Read"
    if leaf in write_verbs:
        return "Write"

    low = desc.lower()
    if "manage " in low or low.startswith("manage "):
        return "Read/Write"
    if any(
        w in low
        for w in [
            "create",
            "delete",
            "edit",
            "update",
            "add ",
            "remove ",
            "configure",
            "run a workflow",
            "switch active",
            "log in",
            "log out",
        ]
    ):
        return "Write"
    if any(w in low for w in ["list", "view", "print", "show", "search", "verify", "display"]):
        return "Read"
    return "Read/Write"


def main() -> None:
    nodes: OrderedDict[str, dict] = OrderedDict()
    visited: set[str] = set()

    def walk(path: list[str], desc: str = "") -> None:
        key = " ".join(path)
        if key in visited:
            return
        visited.add(key)

        subs = parse_subcommands(gh_help(path))
        nodes[key] = {"desc": desc, "subs": [n for n, _ in subs], "depth": len(path)}

        for n, d in subs:
            walk(path + [n], d)

    for name, desc in parse_subcommands(gh_help([])):
        walk([name], desc)

    leaves = [(cmd, meta) for cmd, meta in nodes.items() if not meta["subs"]]

    rows = []
    for cmd, meta in leaves:
        desc = meta["desc"]
        op = classify_operation(cmd, desc)
        rows.append((cmd, meta["depth"], op, desc))

    rows.sort(key=lambda x: x[0])

    md_lines = [
        "# gh leaf command read/write report",
        "",
        "- Discovered by recursively traversing `gh --help` and `gh <...> --help`",
        "- Generated by `python3 portal/scripts/gh-policy-gen.py`",
        "- Only leaf commands are included (no namespace/group commands)",
        "",
        "| Leaf command | Depth | Operation | Description |",
        "|---|---:|---|---|",
    ]

    json_rows = []
    for cmd, depth, op, desc in rows:
        md_lines.append(f"| `gh {cmd}` | {depth} | {op} | {desc} |")
        json_rows.append(
            {
                "command": cmd,
                "depth": depth,
                "operation": op,
                "description": desc,
            }
        )

    md_lines.append("")
    md_lines.append(f"Total leaf commands: **{len(rows)}**")

    out_md = Path("portal/gh-leaf-command-read-write-report.md")
    out_md.write_text("\n".join(md_lines))

    out_json = Path("portal/gh-leaf-command-read-write-report.json")
    out_json.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "source": "gh help recursive traversal",
                "total_leaf_commands": len(rows),
                "commands": json_rows,
            },
            indent=2,
        )
        + "\n"
    )

    print(f"wrote {out_md} with {len(rows)} leaf commands")
    print(f"wrote {out_json} with {len(rows)} leaf commands")


if __name__ == "__main__":
    main()
