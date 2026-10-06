#!/usr/bin/env python3
"""Strict offline gh fixture. JSON protocol is recorded; Git acts only on temp repos."""
import json
import os
from pathlib import Path
import subprocess
import sys

root = Path(os.environ["GH_FIXTURE_DIR"])
state_path = root / "state.json"
state = json.loads(state_path.read_text())
args = sys.argv[1:]
payload = json.loads(sys.stdin.read()) if "--input" in args else None
with (root / "calls.jsonl").open("a") as log:
    log.write(json.dumps({"args": args, "payload": payload, "cwd": os.getcwd()}) + "\n")


def option(name):
    return args[args.index(name) + 1]


def result(value, changed=False):
    if changed:
        state_path.write_text(json.dumps(state))
    print(value if isinstance(value, str) else json.dumps(value))
    sys.exit(0)


def git(*arguments):
    completed = subprocess.run(["git", *arguments], capture_output=True, text=True)
    if completed.returncode:
        print(completed.stderr, file=sys.stderr)
        sys.exit(completed.returncode)
    return completed.stdout.strip()


pr = state["pr"]
if args == ["auth", "status"]:
    result("Fixture authenticated")
if args[:2] == ["repo", "list"]:
    result([{"nameWithOwner": "fixture/repo", "url": "https://example.invalid/fixture/repo", "isPrivate": False}])
if args[:2] == ["repo", "clone"]:
    assert args[2] == "fixture/repo"
    git("clone", "--", state["source"], args[3])
    result("Cloned fixture repository")
if args[:2] == ["pr", "list"]:
    requested = option("--state")
    visible = requested == "all" or pr["state"].lower() == requested
    result([pr] if visible else [])
if args[:2] == ["pr", "view"]:
    assert int(args[2]) == pr["number"]
    result({field: pr[field] for field in option("--json").split(",")})
if args[:2] == ["pr", "checkout"]:
    assert int(args[2]) == pr["number"]
    git("switch", pr["headRefName"])
    result("Checked out fixture branch")
if args[:2] == ["pr", "edit"]:
    assert int(args[2]) == pr["number"]
    pr.update(title=option("--title"), body=option("--body"), baseRefName=option("--base"))
    result("Edited PR fixture", True)
if args[:2] == ["pr", "ready"]:
    assert int(args[2]) == pr["number"]
    pr["isDraft"] = "--undo" in args
    result("Updated draft fixture", True)
if args[:2] == ["pr", "comment"]:
    assert int(args[2]) == pr["number"]
    pr["comments"].append({"author": {"login": "fixture"}, "body": option("--body")})
    result("Commented PR fixture", True)
if args[:2] == ["pr", "create"]:
    assert "--draft" in args
    pr.update(number=8, title=option("--title"), body=option("--body"), baseRefName=option("--base"), headRefName=option("--head"), isDraft=True, state="OPEN")
    pr["headRefOid"] = git("rev-parse", option("--head"))
    result("Created PR fixture", True)
if args[:2] == ["pr", "merge"]:
    assert int(args[2]) == pr["number"]
    assert option("--match-head-commit") == pr["headRefOid"]
    assert pr["state"] == "OPEN"
    assert sum(flag in args for flag in ["--merge", "--squash", "--rebase"]) == 1
    assert "--admin" not in args and "--delete-branch" not in args
    pr["state"] = "MERGED"
    result("Merge fixture complete", True)
if args[0] == "api":
    endpoint = args[1]
    if "/files?" in endpoint:
        assert "--paginate" in args and "--slurp" in args
        result([[state["files"][0]], state["files"][1:]])
    if "/comments?" in endpoint:
        assert "--paginate" in args and "--slurp" in args
        result([state["inline"], []])
    assert option("--method") == "POST" and option("--input") == "-"
    assert payload["commit_id"] == pr["headRefOid"]
    if endpoint.endswith("/reviews"):
        assert payload["event"] in ["APPROVE", "REQUEST_CHANGES", "COMMENT"]
        pr["reviews"].append({"author": {"login": "fixture"}, "body": payload["body"], "state": payload["event"]})
        result({"id": 77, "fixture": "Submitted review fixture"}, True)
    if endpoint.endswith("/comments"):
        assert payload["path"] == state["files"][0]["filename"]
        assert payload["side"] == "RIGHT" and payload["line"] == 6
        state["inline"].append({"user": {"login": "fixture"}, "path": payload["path"], "side": payload["side"], "line": payload["line"], "body": payload["body"], "html_url": "https://example.invalid/inline/88"})
        result({"id": 88, "fixture": "Posted inline fixture"}, True)
print("Unsupported fixture gh call: " + repr(args), file=sys.stderr)
sys.exit(88)
