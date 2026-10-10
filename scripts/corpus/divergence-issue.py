#!/usr/bin/env python3
"""The corpus rounds a master commit fights wrong or refuses that its parent did not, as one issue.

Reads the reports of two corpus runs, this commit's directory and its
parent's, each holding `verify-matches.json` as `verify-matches.py --json-out`
writes it. A round is new when this commit fights it and its result differs
from the match, while the parent fought it as the match says or did not fight
it; or when this commit refuses it, or it does not project onto a layout,
while the parent did not. When there is one or more of either, it writes `issue-title.txt`, `issue.md` and
`count.txt`, the number of new rounds of both kinds, into `--out`; when there is none, or no
parent to compare with, it writes nothing. `--pr` names the pull request the
commit merged, which the issue says brought the rounds.

The issue states what the run saw and how to see it again, and nothing more:
the run has no game, so which mechanism a round parts on is for whoever
reproduces it. The corpus is read at its master, so a new round comes either
from the commit or from a replay added since the parent's run; the issue
names both corpus commits when the runs recorded them.

    python3 scripts/corpus/divergence-issue.py <after> --before <dir> --commit SHA \\
        [--run URL] [--pr N] --out <dir>

`.github/workflows/corpus.yml` runs it on every master commit.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys


def corpus_commit(directory: pathlib.Path) -> str | None:
    path = directory / "corpus-commit"
    return path.read_text().strip() if path.is_file() else None


def rounds(count: int) -> str:
    return f"{count} corpus round{'s' if count != 1 else ''}"


def body(
    new: list[dict],
    refused: list[dict],
    commit: str,
    pr: str | None,
    run: str | None,
    corpus: str | None,
    before: str | None,
) -> str:
    merged = f", which merged #{pr}," if pr else ""
    said = []
    if new:
        said.append(
            f"fights {rounds(len(new))} whose result differs from the match, which its parent"
            " fought as the match says or did not fight"
        )
    if refused:
        said.append(f"refuses {rounds(len(refused))} its parent did not refuse")
    lines = [
        "<!-- corpus-divergence -->",
        f"Master commit {commit}{merged} {', and '.join(said)}.",
        "",
    ]
    if corpus or before:
        lines += [
            f"The corpus was at {corpus or 'an unrecorded commit'} for this run and at"
            f" {before or 'an unrecorded commit'} for the parent's; a round from a replay added"
            " between the two is new to the corpus, not to the simulator.",
            "",
        ]
    if run:
        lines += [f"The run: {run}", ""]
    if new:
        lines += ["| match | round | pinned | differences |", "| --- | ---: | --- | --- |"]
        for found in new:
            leaves = ", ".join(found["differences"])
            lines.append(
                f"| `{found['match']}` | {found['round']} | {'yes' if found['pinned'] else 'no'} | {leaves} |"
            )
        lines.append("")
    if refused:
        lines += ["| match | round | result | reason |", "| --- | ---: | --- | --- |"]
        for found in refused:
            reason = found["reason"].replace("|", "\\|").replace("\n", " ")
            lines.append(f"| `{found['match']}` | {found['round']} | {found['result']} | {reason} |")
        lines.append("")
    lines += [
        "To see one again, after a release build:",
        "",
        "```sh",
        "python3 scripts/corpus/replay.py sync",
        "python3 scripts/corpus/export-replay-corpus.py",
        "target/release/mechcore verify \"work/match/$(cat GAME_VERSION)/<match>\"",
        "```",
        "",
        "A match's last round, which `verify` does not fight, is fought on its own:",
        "",
        "```sh",
        "target/release/mechcore convert \"work/match/$(cat GAME_VERSION)/<match>\" --to layout --round <n> round.yaml",
        "target/release/mechcore convert round.yaml --to mcfr",
        "```",
        "",
        "The first tick a round parts on needs the round recorded by the game, which the run"
        " does not have.",
    ]
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("after", type=pathlib.Path)
    parser.add_argument("--before", type=pathlib.Path)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--run")
    parser.add_argument("--pr")
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()

    if not args.before or not (args.before / "verify-matches.json").is_file():
        print("no parent run to compare with; no issue")
        return 0
    parent = json.loads((args.before / "verify-matches.json").read_text())
    was = {(found["id"], found["round"]) for found in parent["differing"]}
    was_refused = {(found["id"], found["round"]) for found in parent.get("refused", [])}
    summary = json.loads((args.after / "verify-matches.json").read_text())
    new = [found for found in summary["differing"] if (found["id"], found["round"]) not in was]
    refused = [
        found for found in summary["refused"] if (found["id"], found["round"]) not in was_refused
    ]
    if not new and not refused:
        print("no round newly fought wrong or refused; no issue")
        return 0

    args.out.mkdir(parents=True, exist_ok=True)
    short = args.commit[:7]
    said = [f"{len(new)} fought wrong"] * bool(new) + [f"{len(refused)} refused"] * bool(refused)
    (args.out / "issue-title.txt").write_text(f"Corpus rounds newly {' and '.join(said)} at {short}\n")
    (args.out / "issue.md").write_text(
        body(new, refused, short, args.pr, args.run, corpus_commit(args.after), corpus_commit(args.before))
    )
    (args.out / "count.txt").write_text(f"{len(new) + len(refused)}\n")
    print(f"{len(new)} rounds newly fought wrong, {len(refused)} newly refused")
    return 0


if __name__ == "__main__":
    sys.exit(main())
