#!/usr/bin/env python3
"""Fails when a dependency comes from a source the maintainer does not control.

Crates come from crates.io. A git dependency or a [patch] entry may only name
a repository under https://github.com/crmne/ or under the owner of this
repository, and adding or changing one needs the maintainer's approval
(CONTRIBUTING.md, "Pull requests"). This reads every tracked Cargo.toml,
Cargo.lock and .cargo/config.toml.

The repository's owner is GITHUB_REPOSITORY's in CI (the repository the
workflow runs for, also for a pull request from a fork), else the owner of the
checkout's `origin` remote, or of the first remote `git remote` lists, on
GitHub.
"""

import os
import subprocess
import sys
import tomllib
from pathlib import Path
from urllib.parse import urlsplit

CRATES_IO = {
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
}
MANIFESTS = ("/Cargo.toml", "/Cargo.lock", "/.cargo/config.toml", "/.cargo/config")
MAINTAINER = "crmne"


def owned(url, owners=(MAINTAINER,)):
    """True for https://github.com/<owner>/<repository> with one of `owners`, and nothing else."""
    parts = urlsplit(url)
    segments = parts.path.strip("/").removesuffix(".git").split("/")
    return (
        parts.scheme == "https"
        and parts.netloc == "github.com"
        and len(segments) == 2
        and segments[0].casefold() in {owner.casefold() for owner in owners}
        and segments[1] not in ("", ".", "..")
        and "%" not in parts.path
    )


def github_owner(url):
    """The owner of a GitHub repository URL (https, ssh or git@github.com:), else None."""
    if url.startswith("git@github.com:"):
        path = url.removeprefix("git@github.com:")
    else:
        parts = urlsplit(url)
        if parts.scheme not in ("https", "ssh") or parts.hostname != "github.com":
            return None
        path = parts.path
    segments = path.strip("/").removesuffix(".git").split("/")
    if len(segments) != 2 or "%" in path or any(segment in ("", ".", "..") for segment in segments):
        return None
    return segments[0]


def repository_owner(env, remotes):
    """This repository's owner on GitHub: GITHUB_REPOSITORY's in CI, else the owner of the
    `origin` remote or of the first of `remotes` ((name, url) pairs), else None."""
    repository = env.get("GITHUB_REPOSITORY", "")
    if repository:
        owner, _, name = repository.partition("/")
        return owner if owner and name and "/" not in name else None
    urls = dict(remotes)
    if "origin" in urls:
        return github_owner(urls["origin"])
    return github_owner(remotes[0][1]) if remotes else None


def allowed_owners(owner):
    """The maintainer, and the repository's owner when there is one."""
    if owner is None or owner.casefold() == MAINTAINER:
        return (MAINTAINER,)
    return (MAINTAINER, owner)


def git_urls(value, table=()):
    """Yields (table, url) for every git key in a parsed manifest."""
    if isinstance(value, dict):
        for key, item in value.items():
            if key == "git" and isinstance(item, str):
                yield ".".join(table), item
            else:
                yield from git_urls(item, (*table, key))
    elif isinstance(value, list):
        for item in value:
            yield from git_urls(item, table)


def replacements(data):
    """Yields (section, crate, spec) for every [patch] and [replace] entry."""
    for entries in data.get("patch", {}).values():
        for crate, spec in entries.items():
            yield "patch", crate, spec
    for crate, spec in data.get("replace", {}).items():
        yield "replace", crate, spec


def problems(files, owners=(MAINTAINER,)):
    """Returns (path, message) for every disallowed source in {path: TOML text}."""
    found = []
    for path, text in sorted(files.items()):
        data = tomllib.loads(text)
        if path.endswith("Cargo.lock"):
            for package in data.get("package", []):
                source = package.get("source")
                if source and source not in CRATES_IO and not (source.startswith("git+") and owned(source[4:], owners)):
                    found.append((path, f"{package['name']} comes from {source}"))
            continue
        for table, url in git_urls(data):
            if not owned(url, owners):
                found.append((path, f"{table} points at {url}"))
        for section, crate, spec in replacements(data):
            if not (isinstance(spec, dict) and "git" in spec):
                found.append((path, f"[{section}] {crate} must name a git repository under https://github.com/crmne/"))
        for section in ("source", "registries"):
            if section in data:
                found.append((path, f"[{section}] adds a crate source besides crates.io"))
    return found


def tracked_manifests(root):
    listing = subprocess.run(["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True, text=True).stdout
    paths = [path for path in listing.split("\0") if f"/{path}".endswith(MANIFESTS)]
    return {path: (root / path).read_text() for path in paths if (root / path).is_file()}


def git(root, *args):
    return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, text=True).stdout


def remotes(root):
    """The checkout's remotes as (name, url), in the order `git remote` lists them."""
    return [(name, git(root, "remote", "get-url", name).strip()) for name in git(root, "remote").split()]


def main():
    root = Path(__file__).resolve().parents[2]
    owner = repository_owner(os.environ, [] if os.environ.get("GITHUB_REPOSITORY") else remotes(root))
    found = problems(tracked_manifests(root), allowed_owners(owner))
    for path, message in found:
        print(f"::error file={path}::{message}" if os.environ.get("GITHUB_ACTIONS") else f"{path}: {message}")
    if found:
        print(
            "Dependencies come from crates.io or from repositories under https://github.com/crmne/. "
            "Send a dependency fix upstream and ask the maintainer to carry it; see CONTRIBUTING.md."
        )
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
