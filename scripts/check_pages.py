"""Verify a complete Pages tree and inspect it before upload."""

import argparse
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[1]
HELPERS_REVISION = "548597c18f70c010a289c29305c45fe84863d12c"
LIMIT = 64 * 1024 * 1024
ASSET = re.compile(r"assets/[A-Za-z0-9_-]+\.(?:js|css|wasm|svg)")
BASE = "/specter-mirror/"


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.targets = []

    def handle_starttag(self, tag, attributes):
        self.targets.extend(value for key, value in attributes
                            if key in {"src", "href"} and value)


def inventory(site):
    if site.is_symlink() or not site.is_dir():
        raise ValueError("Pages output is missing or linked")
    files = {}
    for path in site.rglob("*"):
        name = path.relative_to(site).as_posix()
        if path.is_symlink():
            raise ValueError("Pages output contains a link")
        if path.is_dir():
            if name != "assets":
                raise ValueError("Unexpected Pages directory")
            continue
        if (not path.is_file() or path.stat().st_size > LIMIT
                or not (name in {"index.html", "about.html", "favicon.svg"}
                        or ASSET.fullmatch(name))):
            raise ValueError("Unexpected or oversized Pages file")
        content = path.read_bytes()
        if name.endswith(".wasm") and not content.startswith(b"\0asm\x01\0\0\0"):
            raise ValueError("Invalid Pages WASM header")
        files[name] = hashlib.sha256(content).hexdigest()
    if not {"index.html", "about.html", "favicon.svg"} <= files.keys():
        raise ValueError("Pages entry documents are missing")
    if not any(name.endswith(".wasm") for name in files):
        raise ValueError("Pages WASM is missing")
    for name in ("index.html", "about.html"):
        links = Links()
        links.feed((site / name).read_text())
        for target in links.targets:
            url = urlsplit(target)
            if url.scheme or url.netloc or not url.path:
                continue
            path = unquote(url.path)
            if path.startswith("/"):
                if not path.startswith(BASE):
                    raise ValueError("Pages project asset base is invalid")
                path = path[len(BASE):]
            elif path.startswith("./"):
                path = path[2:]
            if path not in files:
                raise ValueError("Pages document references a missing file")
    return dict(sorted(files.items()))


def verify(site, manifest, revision=None):
    files = inventory(site)
    if revision is None:
        expected = manifest.read_text()
    else:
        if not re.fullmatch(r"[0-9a-f]{40}", revision):
            raise ValueError("Expected Pages commit is missing or invalid")
        expected = subprocess.check_output(
            ["/usr/bin/git", "--no-replace-objects", "-c", "core.fsmonitor=",
             "-c", "core.hooksPath=/dev/null", "show", f"{revision}:web/pages-manifest.json"],
            cwd=ROOT, env={"PATH": "/usr/bin:/bin", "GIT_CONFIG_NOSYSTEM": "1",
                           "GIT_CONFIG_GLOBAL": "/dev/null"},
            stderr=subprocess.DEVNULL, text=True,
        )
    if json.loads(expected) != files:
        raise ValueError("Pages output differs from the reviewed manifest")
    return files


def inspect(site, helpers, reports, names=None):
    revision = subprocess.check_output(
        ["git", "-C", str(helpers), "rev-parse", "HEAD"], stderr=subprocess.DEVNULL,
        text=True,
    ).strip()
    if revision != HELPERS_REVISION:
        raise ValueError("Pages helper revision differs from the pin")
    dirty = subprocess.check_output(
        ["git", "-C", str(helpers), "status", "--porcelain"], stderr=subprocess.DEVNULL,
        text=True,
    )
    if dirty.strip():
        raise ValueError("Pages helper checkout is modified")
    scripts = helpers / "scripts"
    for name in ("check_publication.sh", "check_private_names.sh"):
        if not (scripts / name).is_file():
            raise ValueError("Pages publication helper is missing")
    for name, version in (("gitleaks", "8.30.1"), ("foxguard", "0.14.1")):
        if not shutil.which(name):
            raise ValueError("Required Pages tool is missing")
        if name == "gitleaks":
            output = subprocess.check_output(["gitleaks", "--version"], stderr=subprocess.STDOUT, text=True)
        else:
            output = subprocess.check_output(["foxguard", "--version"], stderr=subprocess.STDOUT, text=True)
        if not re.search(r"(?<![\d.])v?" + re.escape(version) + r"(?![\d.])", output):
            raise ValueError("Pages tool version differs from the pin")
    snapshot = reports / "site"
    shutil.copytree(site, snapshot)
    files = inventory(snapshot)
    commands = [
        ("Gitleaks", ["dir", str(snapshot), "--no-banner", "--redact"]),
    ]
    config = reports / "analysis.yml"
    config.write_text("{}\n")
    commands.append(("Foxguard", ["--config", str(config),
                                 "--max-file-size", str(LIMIT), str(snapshot)]))
    for name in files:
        commands.append(("Publication", [str(scripts / "check_publication.sh"),
                                         "--file", str(snapshot / name)]))
        if names:
            commands.append(("Publication", [str(scripts / "check_private_names.sh"),
                                             "--list", str(names), "--file", str(snapshot / name),
                                             "--strict-ambiguous"]))
    environment = dict(os.environ, SCRIPT_HELPERS_DIR=str(helpers.resolve()),
                       TMPDIR=str(reports.resolve()))
    for name in ("BASH_ENV", "ENV", "GITLEAKS_CONFIG", "GITLEAKS_CONFIG_TOML"):
        environment.pop(name, None)
    for index, (tool, command) in enumerate(commands):
        with (reports / f"tool-{index}.log").open("wb") as log:
            options = dict(cwd=reports, stdout=log, stderr=subprocess.STDOUT, env=environment)
            if tool == "Gitleaks":
                result = subprocess.run(["gitleaks", *command], **options)
            elif tool == "Foxguard":
                result = subprocess.run(["foxguard", *command], **options)
            elif tool == "Publication":
                result = subprocess.run(["bash", *command], **options)
            else:
                raise ValueError("Unsupported publication operation")
        if result.returncode:
            raise ValueError(f"{tool} stopped publication")
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--record", action="store_true", help="Record locally inspected output digests")
    parser.add_argument("--site", type=Path, default=ROOT / "web/dist")
    parser.add_argument("--manifest", type=Path, default=ROOT / "web/pages-manifest.json")
    parser.add_argument("--helpers", type=Path, default=os.environ.get("SPECTER_PAGES_HELPERS"))
    args = parser.parse_args()
    ci = any(os.environ.get(name, "").lower() in {"true", "1"}
             for name in ("CI", "GITHUB_ACTIONS"))
    reports = None
    try:
        if args.record and ci:
            raise ValueError("Manifest recording is local-only")
        if not args.helpers:
            raise ValueError("Pages publication tools are unavailable")
        names = None
        if args.record:
            names = Path(os.environ.get("PRIVATE_NAMES_FILE", str(
                Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
                / "script-helpers/private-names.tsv")))
            if not names.is_file() or not names.read_bytes().strip():
                raise ValueError("Local publication configuration is missing")
            expected = inventory(args.site)
        else:
            revision = os.environ.get("GITHUB_SHA", "") if ci else None
            expected = verify(args.site, args.manifest, revision)
        base = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir())).resolve()
        if any((directory / ".git").exists() for directory in (base, *base.parents)):
            raise ValueError("Diagnostic storage must be outside Git")
        os.umask(0o077)
        reports = Path(tempfile.mkdtemp(prefix="specter-pages-", dir=base))
        inspected = inspect(args.site, args.helpers, reports, names)
        if inspected != expected or inventory(args.site) != expected:
            raise ValueError("Pages output changed during inspection")
        if args.record:
            args.manifest.write_text(json.dumps(expected, indent=2) + "\n")
            print("Pages manifest recorded.")
        return 0
    except (OSError, ValueError, subprocess.SubprocessError):
        print("Pages publication stopped. Check out this branch locally, run Scan Repository at "
              "https://nikolareljin.github.io/leak-lock/, fix issues and rerun repository tests.")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
