#!/usr/bin/env python3
"""Publish, fetch and delete a research question's oracle: the recordings it is answered against.

    scripts/oracle.py publish <n> <path>...  upload files under /tmp/mechcore to the release oracle/issue-<n>
    scripts/oracle.py fetch <n> [--force]    download the release oracle/issue-<n> back under /tmp/mechcore
    scripts/oracle.py retract <n> <path>...  take files back out of the release oracle/issue-<n>
    scripts/oracle.py delete <n>             delete the release and its tag, when the issue closes
    scripts/oracle.py list [<n>]             list the oracles, or one oracle's files

A recording never enters the repository; what the repository keeps is the
script that records it. A claimant answering a research question has no game
to run that script against, so the keeper publishes what it recorded as a
release of this repository named after the question's issue, and the claimant
fetches it to the paths the script would have written:
`/tmp/mechcore/<topic>/<script>/<name>.mcfr`, and the sidecar beside it. A
release is not a commit, is added to when a capture is asked for, loses a
recording the keeper recorded around a blocker, and is deleted when the issue
closes. `.github/CONTRIBUTING.md`'s Research section is where an oracle fits
in.

An asset name cannot hold a slash, so the path under `/tmp/mechcore` is joined
with `__`. `MANIFEST.json` lists every file with its size and SHA-256, and
`fetch` verifies each one and refuses to overwrite a file that differs unless
told `--force`. Publishing and deleting need `gh` signed in; fetching uses
`gh` when it is and plain HTTPS otherwise, because the release is public.
"""

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

REPOSITORY = "qbisi/mechcore"
ROOT = Path("/tmp/mechcore")
REAL_ROOT = ROOT.resolve()  # /tmp is a link to /private/tmp on macOS
SEPARATOR = "__"
MANIFEST = "MANIFEST.json"


def fail(message):
    print(f"oracle: {message}", file=sys.stderr)
    sys.exit(1)


def gh(*arguments, check=True):
    result = subprocess.run(["gh", *arguments], text=True, capture_output=True)
    if check and result.returncode != 0:
        fail(f"gh {' '.join(arguments)} failed:\n{result.stderr or result.stdout}")
    return result


def gh_signed_in():
    if shutil.which("gh") is None:
        return False
    return subprocess.run(["gh", "auth", "status"], capture_output=True).returncode == 0


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def release(issue):
    return f"oracle/issue-{int(issue)}"


def asset_name(relative):
    return SEPARATOR.join(relative.parts)


def relative_path(name):
    return Path(*name.split(SEPARATOR))


def publish(issue, paths):
    files = []
    for argument in paths:
        path = Path(argument).resolve()
        if path.is_dir():
            files.extend(sorted(p for p in path.rglob("*") if p.is_file()))
        elif path.is_file():
            files.append(path)
        else:
            fail(f"{argument} is neither a file nor a directory")
    if not files:
        fail("nothing to publish")
    entries = {}
    located = []
    for path in files:
        try:
            relative = path.relative_to(REAL_ROOT)
        except ValueError:
            fail(f"{path} is not under {ROOT}; a script writes there, and an oracle keeps its paths")
        entries[str(relative)] = {"bytes": path.stat().st_size, "sha256": sha256(path)}
        located.append((relative, path))
    tag = release(issue)
    existing = gh("release", "view", tag, "--repo", REPOSITORY, "--json", "assets", check=False)
    if existing.returncode == 0:
        # A capture request adds to an oracle; the manifest has to keep what
        # is already there.
        with tempfile.TemporaryDirectory() as scratch:
            gh("release", "download", tag, "--repo", REPOSITORY, "--dir", scratch, "--pattern", MANIFEST)
            previous = json.loads((Path(scratch) / MANIFEST).read_text())["files"]
        entries = {**previous, **entries}
    with tempfile.TemporaryDirectory() as scratch:
        manifest = Path(scratch) / MANIFEST
        manifest.write_text(json.dumps({"root": str(ROOT), "files": entries}, indent=2, sort_keys=True) + "\n")
        uploads = [manifest]
        for relative, path in located:
            link = Path(scratch) / asset_name(relative)
            os.symlink(path, link)
            uploads.append(link)
        if existing.returncode != 0:
            gh(
                "release", "create", tag, "--repo", REPOSITORY,
                "--title", tag,
                "--notes", f"The recordings research issue #{int(issue)} is answered against. "
                "`scripts/oracle.py fetch` puts them back under /tmp/mechcore; "
                ".github/CONTRIBUTING.md (Research) says how they are used. "
                "Deleted when the issue closes.",
            )
        gh("release", "upload", tag, "--repo", REPOSITORY, "--clobber", *map(str, uploads))
    for relative, entry in sorted(entries.items()):
        print(f"{entry['sha256']}  {entry['bytes']:>10}  {relative}")
    print(f"{tag} holds {len(entries)} file(s)")


def download(tag, name, destination):
    url = f"https://github.com/{REPOSITORY}/releases/download/{tag.replace('/', '%2F')}/{name}"
    try:
        with urllib.request.urlopen(url) as response, open(destination, "wb") as handle:
            shutil.copyfileobj(response, handle)
    except OSError as error:
        fail(f"cannot download {url}: {error}")


def fetch(issue, force):
    tag = release(issue)
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        if gh_signed_in():
            gh("release", "download", tag, "--repo", REPOSITORY, "--dir", str(scratch))
        else:
            download(tag, MANIFEST, scratch / MANIFEST)
        manifest_path = scratch / MANIFEST
        if not manifest_path.exists():
            fail(f"{tag} carries no {MANIFEST}; it was not published by this script")
        manifest = json.loads(manifest_path.read_text())
        for relative, entry in sorted(manifest["files"].items()):
            source = scratch / asset_name(Path(relative))
            if not source.exists():
                download(tag, asset_name(Path(relative)), source)
            digest = sha256(source)
            if digest != entry["sha256"]:
                fail(f"{relative} downloaded with SHA-256 {digest}, manifest says {entry['sha256']}")
            target = ROOT / relative
            if target.exists() and sha256(target) != digest:
                if not force:
                    fail(f"{target} exists and differs; pass --force to replace it")
                target.unlink()
            target.parent.mkdir(parents=True, exist_ok=True)
            if not target.exists():
                shutil.move(str(source), target)
            print(f"{digest}  {target}")
    print(f"fetched {len(manifest['files'])} file(s) from {tag}")


def retract(issue, paths):
    """Takes recordings back out of an oracle: a fixture the keeper recorded
    around a blocker is no longer one the question is answered against, and
    CI plays back every recording the release holds."""
    tag = release(issue)
    relatives = []
    for argument in paths:
        path = Path(argument)
        if path.is_absolute():
            try:
                path = path.resolve().relative_to(REAL_ROOT)
            except ValueError:
                fail(f"{argument} is not under {ROOT}")
        relatives.append(str(path))
    with tempfile.TemporaryDirectory() as scratch:
        gh("release", "download", tag, "--repo", REPOSITORY, "--dir", scratch, "--pattern", MANIFEST)
        manifest_path = Path(scratch) / MANIFEST
        manifest = json.loads(manifest_path.read_text())
        missing = [relative for relative in relatives if relative not in manifest["files"]]
        if missing:
            fail(f"{tag} does not hold {', '.join(missing)}")
        for relative in relatives:
            del manifest["files"][relative]
        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        gh("release", "upload", tag, "--repo", REPOSITORY, "--clobber", str(manifest_path))
    for relative in relatives:
        gh("release", "delete-asset", tag, asset_name(Path(relative)), "--repo", REPOSITORY, "--yes")
        print(f"retracted {relative}")
    print(f"{tag} holds {len(manifest['files'])} file(s)")


def delete(issue):
    tag = release(issue)
    gh("release", "delete", tag, "--repo", REPOSITORY, "--cleanup-tag", "--yes")
    print(f"deleted {tag} and its tag")


def list_oracles(issue):
    if issue is None:
        result = gh("api", f"repos/{REPOSITORY}/releases?per_page=100")
        for entry in json.loads(result.stdout):
            if entry["tag_name"].startswith("oracle/"):
                print(f"{entry['published_at']}  {entry['tag_name']}")
        return
    result = gh("release", "view", release(issue), "--repo", REPOSITORY, "--json", "assets")
    for asset in json.loads(result.stdout)["assets"]:
        name = asset["name"]
        shown = MANIFEST if name == MANIFEST else str(relative_path(name))
        print(f"{asset['size']:>10}  {shown}")


def main(argv):
    if len(argv) >= 4 and argv[1] == "publish":
        publish(argv[2], argv[3:])
    elif len(argv) >= 3 and argv[1] == "fetch":
        fetch(argv[2], "--force" in argv[3:])
    elif len(argv) >= 4 and argv[1] == "retract":
        retract(argv[2], argv[3:])
    elif len(argv) == 3 and argv[1] == "delete":
        delete(argv[2])
    elif len(argv) in (2, 3) and argv[1] == "list":
        list_oracles(argv[2] if len(argv) == 3 else None)
    else:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv)
