#!/usr/bin/env python3
"""Compare upstream TidaLuna's exported plugin API with Tidalfast's Luna shim.

Run it when the luna-watch mail reports a new TidaLuna commit or release:

    python scripts/luna-api-diff.py                 # clones TidaLuna into .cache/
    python scripts/luna-api-diff.py --upstream DIR  # an existing checkout
    python scripts/luna-api-diff.py --markdown      # rewrite the generated table

Upstream side: the exports of `@luna/core` (render/src/index.ts), `@luna/lib`
(plugins/lib/src/index.ts), `@luna/ui` (plugins/ui/src/index.tsx) and
`@luna/lib.native` (plugins/lib.native/src/index.native.ts), followed through
`export * from` and `export * as`, plus the public members of exported classes.
Shim side: `src/luna/api-surface.json`, which the test
`luna::api_tests::the_shim_api_surface_matches_the_checked_in_list` writes
from the running runtime (`LUNA_API_SURFACE_UPDATE=1 cargo test api_surface`)
and fails on when it is stale.

Members a class sets per instance are checked against one instance of each
(`#instances` in the surface list); the redux store state against upstream's
`TidalStoreState` types (`#state`); and `src/luna/action-types.txt`
(`redux.actionTypes`) against upstream's `actionTypes.ts`, which `--markdown`
rewrites with the table. `--rev` names the upstream commit for a checkout that
is not a git clone (a release tarball).

The report lists what upstream exports and the shim lacks (plugins using it
get a logged warning and an inert stand-in, see luna-compat.md "Upgrading
Luna"), what the shim has that upstream dropped, and whether the upstream
version differs from `src/luna/versions.rs`. Exit status 1 when anything
upstream is missing from the shim.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SURFACE = ROOT / "src" / "luna" / "api-surface.json"
VERSIONS = ROOT / "src" / "luna" / "versions.rs"
TABLE = ROOT / "docs" / "_reference" / "luna-api-surface.md"
UPSTREAM_URL = "https://github.com/Inrixia/TidaLuna"

ENTRIES = {
    "@luna/core": "render/src/index.ts",
    "@luna/lib": "plugins/lib/src/index.ts",
    "@luna/ui": "plugins/ui/src/index.tsx",
    "@luna/lib.native": "plugins/lib.native/src/index.native.ts",
}

EXPORT_STAR = re.compile(r'^export\s+\*\s+from\s+["\'](.+?)["\']', re.M)
EXPORT_STAR_AS = re.compile(r'^export\s+\*\s+as\s+(\w+)\s+from\s+["\'](.+?)["\']', re.M)
EXPORT_NAMED = re.compile(r"^export\s+(?:type\s+)?\{([^}]*)\}(?:\s+from\s+[\"'](.+?)[\"'])?", re.M)
EXPORT_DECL = re.compile(
    r"^export\s+(?:declare\s+)?(?:default\s+)?(?:abstract\s+)?(?:async\s+)?"
    r"(const|let|var|function\*?|class|enum|type|interface|namespace)\s+(\{[^}]*\}|\w+)",
    re.M,
)
MEMBER = re.compile(
    r"^\t(?!private\b|protected\b|#|//|\*|/\*)"
    r"((?:public\s+|static\s+|readonly\s+|async\s+|override\s+|get\s+|set\s+|\*\s*)*)"
    r"(\w+)\s*[(<:=?!]"
)


def resolve(base: Path, spec: str) -> Path | None:
    if not spec.startswith("."):
        return None
    target = (base.parent / spec).resolve()
    for cand in (
        target,
        target.with_name(target.name + ".ts"),
        target.with_name(target.name + ".tsx"),
        target / "index.ts",
        target / "index.tsx",
    ):
        if cand.is_file():
            return cand
    return None


def class_members(text: str, name: str) -> list[str]:
    m = re.search(rf"^export\s+(?:abstract\s+)?class\s+{name}\b[^\n]*\{{\s*$", text, re.M)
    if not m:
        return []
    out: set[str] = set()
    for line in text[m.end():].splitlines():
        if line.startswith("}"):
            break
        mm = MEMBER.match(line)
        if mm and mm.group(2) not in ("constructor",):
            static = "static" in mm.group(1)
            out.add(("static " if static else "") + mm.group(2))
    return sorted(out)


def module_exports(path: Path, seen: set[Path] | None = None) -> dict[str, dict]:
    """name -> {"kind": ..., "members": [...]} for one module, `export *` followed."""
    seen = seen if seen is not None else set()
    if path in seen:
        return {}
    seen.add(path)
    text = path.read_text(encoding="utf-8")
    out: dict[str, dict] = {}
    for m in EXPORT_STAR_AS.finditer(text):
        sub = resolve(path, m.group(2))
        out[m.group(1)] = {
            "kind": "namespace",
            "members": sorted(module_exports(sub, set()).keys()) if sub else [],
        }
    for m in EXPORT_STAR.finditer(text):
        sub = resolve(path, m.group(1))
        if sub:
            for k, v in module_exports(sub, seen).items():
                out.setdefault(k, v)
    for m in EXPORT_NAMED.finditer(text):
        if re.match(r"^export\s+type\s", m.group(0)):
            continue
        sub = resolve(path, m.group(2)) if m.group(2) else None
        sub_exports = module_exports(sub, set()) if sub else {}
        for part in m.group(1).split(","):
            part = part.strip()
            if not part or part.startswith("type "):
                continue
            original = part.split(" as ")[0].strip()
            alias = part.split(" as ")[-1].strip()
            if sub and original not in sub_exports:
                # `export { MediaFormat } from "./MediaItem"` where the module
                # declares `export type MediaFormat`: a type, not a value.
                continue
            out.setdefault(alias, sub_exports.get(original, {"kind": "value", "members": []}))
    for m in EXPORT_DECL.finditer(text):
        kind, name = m.group(1), m.group(2)
        if kind in ("type", "interface"):
            continue
        if name.startswith("{"):
            for part in name.strip("{} ").split(","):
                alias = part.split(":")[-1].strip()
                if alias:
                    out.setdefault(alias, {"kind": "value", "members": []})
            continue
        kind = {"function*": "function", "let": "value", "var": "value", "const": "value"}.get(kind, kind)
        members = class_members(text, name) if kind == "class" else []
        out[name] = {"kind": kind, "members": members}
    return out


STORE_TYPES = "plugins/lib/src/redux/types/store"
ACTION_TYPES_TS = "plugins/lib/src/redux/types/actions/actionTypes.ts"
ACTION_TYPES = ROOT / "src" / "luna" / "action-types.txt"


def interfaces(up: Path) -> dict[str, list[str]]:
    """Every `export interface Name { field: ... }` of @luna/lib's store types."""
    out: dict[str, list[str]] = {}
    for f in sorted((up / STORE_TYPES).rglob("*.ts")):
        text = f.read_text(encoding="utf-8")
        for m in re.finditer(r"^export\s+interface\s+(\w+)(?:<[^>]*>)?(?:\s+extends\s+[^{]+)?\s*\{\s*$", text, re.M):
            fields = []
            for line in text[m.end():].splitlines():
                if line.startswith("}"):
                    break
                fm = re.match(r"^\t(\w+)\??:", line)
                if fm:
                    fields.append(fm.group(1))
            out[m.group(1)] = fields
    return out


def state_gaps(up: Path, shim_state: dict) -> list[str]:
    """Store state slices and fields upstream types declare that the shim's
    `getState()` lacks."""
    ifaces = interfaces(up)
    index = (up / STORE_TYPES / "index.ts").read_text(encoding="utf-8")
    m = re.search(r"^export interface TidalStoreState \{(.*?)^\}", index, re.M | re.S)
    if not m:
        return ["TidalStoreState not found upstream"]
    gaps = []
    for line in m.group(1).splitlines():
        fm = re.match(r"^\t(\w+)\??:\s*(.+?);?$", line)
        if not fm:
            continue
        slice_, typ = fm.group(1), fm.group(2).strip()
        if slice_ not in shim_state:
            gaps.append(f"state.{slice_}")
            continue
        fields = ifaces.get(typ, [])
        if typ == "{":  # inline object type
            fields = []
        for field in fields:
            if field not in shim_state[slice_]:
                gaps.append(f"state.{slice_}.{field}")
    return gaps


def upstream_action_types(up: Path) -> list[str]:
    text = (up / ACTION_TYPES_TS).read_text(encoding="utf-8")
    body = text[text.index("actionTypes = ["):]
    return re.findall(r'"([^"]+)"', body[: body.index("]")])


def upstream_version(up: Path) -> str:
    try:
        return json.loads((up / "package.json").read_text(encoding="utf-8")).get("version", "?")
    except OSError:
        return "?"


def shim_version() -> str:
    m = re.search(r'pub const LUNA_API: &str = "([^"]+)"', VERSIONS.read_text(encoding="utf-8"))
    return m.group(1) if m else "?"


def ensure_upstream(arg: str | None) -> Path:
    if arg:
        return Path(arg)
    dest = ROOT / ".cache" / "tidaluna-upstream"
    if (dest / ".git").is_dir():
        subprocess.run(["git", "-C", str(dest), "pull", "--ff-only", "-q"], check=True)
    else:
        dest.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "clone", "--depth", "1", "-q", UPSTREAM_URL, str(dest)], check=True)
    return dest


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--upstream", help="an existing TidaLuna checkout (default: clone into .cache/)")
    ap.add_argument("--markdown", action="store_true", help=f"rewrite {TABLE.relative_to(ROOT)} and {ACTION_TYPES.relative_to(ROOT)}")
    ap.add_argument("--rev", help="upstream commit, when --upstream is not a git checkout (a release tarball)")
    args = ap.parse_args()

    up = ensure_upstream(args.upstream)
    shim = json.loads(SURFACE.read_text(encoding="utf-8"))
    instances = shim.get("#instances", {})
    rev = args.rev or subprocess.run(["git", "-C", str(up), "rev-parse", "--short", "HEAD"], capture_output=True, text=True).stdout.strip() or "?"

    rows = []
    missing = 0
    for module, entry in ENTRIES.items():
        path = up / entry
        upstream = module_exports(path) if path.is_file() else {}
        ours = shim.get(module, {})
        for name in sorted(set(upstream) | set(ours)):
            u, s = upstream.get(name), ours.get(name)
            if u and s is None:
                status, note = "missing", "warns and stands in"
                missing += 1
            elif s is not None and not u:
                status, note = "extra", "Tidalfast only, or dropped upstream"
            else:
                ours_m = set(s.get("members", []))
                if s.get("kind") != "class":
                    # An object of functions stands for a class of statics.
                    ours_m |= {"static " + m for m in ours_m}
                # Members the shim sets per instance are checked against one
                # instance of the class (`#instances` in the surface list).
                ours_m |= set(instances.get(f"{module} {name}", []))
                gaps = sorted(set(u["members"]) - ours_m)
                checked = f"{module} {name}" in instances
                hard = [g for g in gaps if g.startswith("static ") or u["kind"] == "namespace" or checked]
                soft = [g for g in gaps if g not in hard]
                status = "partial" if hard else "yes"
                notes = []
                if hard:
                    notes.append("no " + ", ".join(hard))
                if soft:
                    notes.append("instance members not on the prototype (check they are set per instance): " + ", ".join(soft))
                note = "; ".join(notes)
                if hard:
                    missing += 1
            rows.append((module, name, (u or s or {}).get("kind", "value"), status, note))

    uv, sv = upstream_version(up), shim_version()
    print(f"upstream TidaLuna {uv} ({rev}); shim follows {sv}")
    if uv != sv:
        print(f"  version differs: update LUNA_API in src/luna/versions.rs once the gaps below are handled")
    for module, name, kind, status, note in rows:
        if status in ("missing", "partial"):
            print(f"  {status:8} {module} {name} ({kind}) {note}")
    extras = [r for r in rows if r[3] == "extra"]
    if extras:
        print(f"  {len(extras)} shim exports not in upstream (see the table)")

    # The store state plugins read through redux.store.getState().
    sgaps = state_gaps(up, shim.get("#state", {})) if (up / STORE_TYPES).is_dir() else []
    for g in sgaps:
        print(f"  missing  {g} (redux store state)")
    missing += len(sgaps)

    # redux.actionTypes: the list TIDAL's store knows.
    action_types = upstream_action_types(up) if (up / ACTION_TYPES_TS).is_file() else []
    ours_at = [l for l in ACTION_TYPES.read_text(encoding="utf-8").splitlines() if l] if ACTION_TYPES.is_file() else []
    if action_types and action_types != ours_at:
        added, dropped = set(action_types) - set(ours_at), set(ours_at) - set(action_types)
        print(f"  stale    {ACTION_TYPES.relative_to(ROOT)}: {len(added)} new, {len(dropped)} dropped upstream (--markdown rewrites it)")
        if not args.markdown:
            missing += 1
    if args.markdown:
        lines = [
            "# Luna API surface: upstream vs Tidalfast's shim",
            "",
            "Generated by `python scripts/luna-api-diff.py --markdown`; do not edit by hand.",
            f"Upstream TidaLuna {uv} (`{rev}`), shim follows {sv} (`src/luna/versions.rs`).",
            "",
            "`yes`: present with every public member upstream has. `partial`: present, the",
            "listed members missing. `missing`: not exported by the shim (a plugin reading it",
            "gets a logged warning and an inert stand-in). `extra`: only in the shim.",
            "",
            "| Module | Export | Kind | In shim | Notes |",
            "|---|---|---|---|---|",
        ]
        for module, name, kind, status, note in rows:
            lines.append(f"| `{module}` | `{name}` | {kind} | {status} | {note} |")
        lines += [
            "",
            "## Store state",
            "",
            "Slices and fields of `redux.store.getState()` that upstream's `TidalStoreState`",
            "types declare and the shim lacks: " + (", ".join(f"`{g}`" for g in sgaps) if sgaps else "none."),
            "",
            f"`redux.actionTypes`: {len(ours_at if not args.markdown else action_types)} action types",
            f"(`{ACTION_TYPES.relative_to(ROOT).as_posix()}`, generated from upstream's `actionTypes.ts`).",
        ]
        TABLE.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
        print(f"wrote {TABLE.relative_to(ROOT)}")
        if action_types and action_types != ours_at:
            ACTION_TYPES.write_text("\n".join(action_types) + "\n", encoding="utf-8", newline="\n")
            print(f"wrote {ACTION_TYPES.relative_to(ROOT)}")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
