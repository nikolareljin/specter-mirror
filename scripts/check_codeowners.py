import argparse
from pathlib import Path


def valid_default_owner(text, owner):
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
    if not valid_default_owner(args.path.read_text(), args.owner):
        raise SystemExit("CODEOWNERS must retain the single default-owner rule.")


if __name__ == "__main__":
    main()
