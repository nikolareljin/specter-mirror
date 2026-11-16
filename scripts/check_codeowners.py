import argparse
from pathlib import Path


MAX_CODEOWNERS_BYTES = 3_000_000


def valid_default_owner(text, owner):
    if len(text.encode("utf-8")) >= MAX_CODEOWNERS_BYTES:
        return False
    rules = [
        line.split() for line in text.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    return rules == [["*", "@" + owner]]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("path", type=Path)
    parser.add_argument("--owner", required=True)
    args = parser.parse_args()
    with args.path.open("rb") as stream:
        content = stream.read(MAX_CODEOWNERS_BYTES)
    if len(content) >= MAX_CODEOWNERS_BYTES:
        raise SystemExit("CODEOWNERS must remain below 3 MB.")
    try:
        text = content.decode("utf-8")
    except UnicodeDecodeError:
        raise SystemExit("CODEOWNERS must use UTF-8.")
    if not valid_default_owner(text, args.owner):
        raise SystemExit("CODEOWNERS must retain the single default-owner rule.")


if __name__ == "__main__":
    main()
