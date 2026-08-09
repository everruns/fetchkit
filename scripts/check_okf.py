#!/usr/bin/env python3
"""Validate Fetchkit's Open Knowledge Format (OKF) v0.2 bundle.

The upstream specification intentionally keeps most structure optional. This
checker enforces the bundle-local contract in knowledge/knowledge-contract.md:
metadata, complete indexes, dated logs, resolvable graph links, and sound
metadata for generated concepts. It has no third-party Python dependencies.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

RESERVED = ("index.md", "log.md")
DATE_HEADING = re.compile(r"^## \d{4}-\d{2}-\d{2}\s*$")
LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
EXTERNAL = re.compile(r"\A(?:[a-z][a-z0-9+.-]*:|//|#)")
DATE = re.compile(r"\A\d{4}-\d{2}-\d{2}\Z")
ACTOR = re.compile(r"\A(?:human:\S+|process:\S+|[^/\s]+/[^/\s]+)\Z")
STATUSES = ("draft", "stable", "deprecated")
BUNDLE_PATH = re.compile(r"knowledge/[A-Za-z0-9_./-]+\.(?:md|json)")
CODE = re.compile(r"^```.*?^```|``.*?``|`[^`\n]*`", re.DOTALL | re.MULTILINE)


def strip_code(text: str) -> str:
    """Blank code spans and fences so their examples are not parsed as links."""
    return CODE.sub(lambda match: "\n" * match.group(0).count("\n"), text)


def split_frontmatter(text: str) -> tuple[str | None, str]:
    """Return frontmatter and body, or None and the full text when absent."""
    if not text.startswith("---\n"):
        return None, text
    end = text.find("\n---\n", 3)
    if end == -1:
        raise ValueError("unterminated frontmatter block")
    return text[4:end], text[end + 5 :]


def parse_frontmatter(frontmatter: str) -> dict[str, object]:
    """Parse the scalar, scalar-list, and one-level mapping subset in use."""
    data: dict[str, object] = {}
    key: str | None = None
    for lineno, raw in enumerate(frontmatter.splitlines(), start=2):
        line = raw.rstrip()
        if not line or line.lstrip().startswith("#"):
            continue
        if line.startswith("  "):
            if key is None:
                raise ValueError(f"line {lineno}: indented entry without a key")
            item = line.strip()
            if item.startswith("- "):
                existing = data.get(key)
                values = existing if isinstance(existing, list) else []
                data[key] = values + [item[2:].strip()]
            elif ":" in item:
                nested = data.get(key)
                if not isinstance(nested, dict):
                    nested = {}
                    data[key] = nested
                subkey, _, value = item.partition(":")
                nested[subkey.strip()] = value.strip()
            else:
                raise ValueError(f"line {lineno}: unparseable entry {item!r}")
            continue
        if ":" not in line:
            raise ValueError(f"line {lineno}: unparseable key line {line!r}")
        key, _, value = line.partition(":")
        key = key.strip()
        data[key] = value.strip()
    return data


def index_targets(path: pathlib.Path) -> set[str]:
    if not path.exists():
        return set()
    _, body = split_frontmatter(path.read_text())
    return {
        target.split("#", 1)[0].rstrip("/")
        for target in LINK.findall(strip_code(body))
    }


def check_links(path: pathlib.Path, rel: str, errors: list[str]) -> None:
    _, body = split_frontmatter(path.read_text())
    for target in LINK.findall(strip_code(body)):
        if EXTERNAL.match(target):
            continue
        resolved = target.split("#", 1)[0]
        if resolved and not (path.parent / resolved).exists():
            errors.append(f"{rel}: link target does not exist: {target}")


def check_trust(
    path: pathlib.Path,
    rel: str,
    metadata: dict[str, object],
    errors: list[str],
) -> None:
    generated = metadata.get("generated")
    resource = metadata.get("resource")
    if metadata.get("type") == "Generated Inventory":
        if not isinstance(generated, dict) or not generated.get("by"):
            errors.append(
                f"{rel}: a 'Generated Inventory' must declare 'generated.by'"
            )
        if not resource:
            errors.append(f"{rel}: a 'Generated Inventory' must declare 'resource'")
    if isinstance(generated, dict):
        actor = generated.get("by")
        if actor and not ACTOR.match(str(actor)):
            errors.append(f"{rel}: 'generated.by' is not an OKF actor: {actor!r}")
    if isinstance(resource, str) and resource and not EXTERNAL.match(resource):
        if not (path.parent / resource).exists():
            errors.append(f"{rel}: 'resource' does not exist: {resource}")
    status = metadata.get("status")
    if status and status not in STATUSES:
        errors.append(f"{rel}: 'status' must be one of {STATUSES}, got {status!r}")
    stale_after = metadata.get("stale_after")
    if stale_after and not DATE.match(str(stale_after)):
        errors.append(
            f"{rel}: 'stale_after' must be YYYY-MM-DD, got {stale_after!r}"
        )


def check_cross_links(
    path: pathlib.Path, rel: str, body: str, errors: list[str]
) -> None:
    for target in LINK.findall(strip_code(body)):
        if EXTERNAL.match(target):
            continue
        resolved = (path.parent / target.split("#", 1)[0]).resolve()
        if (
            resolved.suffix == ".md"
            and resolved.name not in RESERVED
            and resolved != path.resolve()
            and resolved.exists()
        ):
            return
    errors.append(f"{rel}: links to no other concept")


def check_concept(path: pathlib.Path, rel: str, errors: list[str]) -> None:
    text = path.read_text()
    frontmatter, body = split_frontmatter(text)
    if frontmatter is None:
        errors.append(f"{rel}: missing YAML frontmatter block")
        return
    metadata = parse_frontmatter(frontmatter)
    if not metadata.get("type"):
        errors.append(f"{rel}: frontmatter must contain a non-empty 'type'")
    for field in ("title", "description"):
        if not metadata.get(field):
            errors.append(f"{rel}: frontmatter must contain a non-empty '{field}'")
    if "summary" in metadata:
        errors.append(f"{rel}: 'summary' is not an OKF field; use 'description'")
    check_trust(path, rel, metadata, errors)
    check_cross_links(path, rel, body, errors)


def check_index(path: pathlib.Path, rel: str, is_root: bool, errors: list[str]) -> None:
    frontmatter, body = split_frontmatter(path.read_text())
    if frontmatter is not None:
        keys = set(parse_frontmatter(frontmatter))
        allowed = {"okf_version"} if is_root else set()
        extra = sorted(keys - allowed)
        if extra:
            errors.append(f"{rel}: index.md may not carry frontmatter keys {extra}")
    if not body.strip():
        errors.append(f"{rel}: index.md body is empty")


def check_log(path: pathlib.Path, rel: str, errors: list[str]) -> None:
    frontmatter, body = split_frontmatter(path.read_text())
    if frontmatter is not None:
        errors.append(f"{rel}: log.md may not carry frontmatter")
    headings = [line for line in body.splitlines() if line.startswith("## ")]
    if not headings:
        errors.append(f"{rel}: log.md needs at least one '## YYYY-MM-DD' heading")
    for heading in headings:
        if not DATE_HEADING.match(heading):
            errors.append(f"{rel}: log heading {heading!r} is not '## YYYY-MM-DD'")


def check_bundle_paths(rel: str, text: str, errors: list[str]) -> None:
    for match in sorted(set(BUNDLE_PATH.findall(text))):
        errors.append(
            f"{rel}: reference bundle documents as relative markdown links, "
            f"not as repository paths: {match}"
        )


def check_bundle(root: pathlib.Path) -> tuple[list[str], dict[str, int]]:
    errors: list[str] = []
    counts = {"concepts": 0, "indexes": 0, "logs": 0}
    if not (root / "index.md").exists():
        errors.append("index.md: bundle root index is missing")

    directories = sorted(path for path in root.rglob("*") if path.is_dir()) + [root]
    for directory in directories:
        listed = index_targets(directory / "index.md")
        for child in sorted(directory.iterdir()):
            rel = child.relative_to(root).as_posix()
            if child.is_dir():
                if not (child / "index.md").exists():
                    errors.append(f"{rel}/: subdirectory has no index.md")
                if child.name not in listed:
                    errors.append(f"{rel}/: not listed in {directory.name}/index.md")
                continue
            if child.suffix != ".md" or child.name in RESERVED:
                continue
            if child.name not in listed:
                index_rel = (directory / "index.md").relative_to(root).as_posix()
                errors.append(f"{rel}: not listed in {index_rel}")

    for path in sorted(root.rglob("*.md")):
        rel = path.relative_to(root).as_posix()
        try:
            if path.name == "index.md":
                counts["indexes"] += 1
                check_index(path, rel, path.parent == root, errors)
            elif path.name == "log.md":
                counts["logs"] += 1
                check_log(path, rel, errors)
            else:
                counts["concepts"] += 1
                check_concept(path, rel, errors)
            check_links(path, rel, errors)
            check_bundle_paths(rel, path.read_text(), errors)
        except ValueError as error:
            errors.append(f"{rel}: {error}")

    return errors, counts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", nargs="?", default="knowledge", type=pathlib.Path)
    args = parser.parse_args(argv)
    if not args.bundle.is_dir():
        print(f"error: {args.bundle} is not a directory", file=sys.stderr)
        return 2

    errors, counts = check_bundle(args.bundle)
    if errors:
        print(
            f"{args.bundle}: {len(errors)} OKF conformance error(s)",
            file=sys.stderr,
        )
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1

    print(
        f"{args.bundle}: OKF v0.2 conformant ({counts['concepts']} concepts, "
        f"{counts['indexes']} index files, {counts['logs']} log file"
        f"{'s' if counts['logs'] != 1 else ''})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
