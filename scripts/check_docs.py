from pathlib import Path
import re

root = Path(__file__).resolve().parents[1]
errors = []
for folder in (root / "docs", root / ".kiro"):
    for path in folder.rglob("*.md"):
        for target in re.findall(r"\]\(([^)]+)\)", path.read_text()):
            if ":" in target or target.startswith("#"):
                continue
            if not (path.parent / target.split("#", 1)[0]).exists():
                errors.append(str(path.relative_to(root)))
if errors:
    raise SystemExit("Broken documentation links: " + ", ".join(sorted(set(errors))))
print("Documentation links checked. Review build, privacy and evidence docs when behavior changes.")
