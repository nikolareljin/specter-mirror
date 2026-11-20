"""Add a verified provisioning toolkit to bootable antiX installation media."""

import argparse
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("appliance", ROOT / "scripts/appliance/manage.py")
appliance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(appliance)


def build(args):
    base = args.base.resolve(strict=True)
    if appliance.digest(base) != args.base_sha256.lower():
        raise ValueError("Base ISO checksum mismatch")
    report = subprocess.run(["xorriso", "-indev", str(base), "-report_el_torito", "plain"],
                            capture_output=True, text=True, check=True)
    if not re.search(r"El Torito boot img", report.stdout + report.stderr):
        raise ValueError("Base ISO has no bootable El Torito image")
    appliance.check_bundle(args.bundle, args.bundle_sha256)
    if not args.allow_proof:
        raise ValueError("Only synthetic-proof media is supported; --allow-proof is required")
    if args.output.exists():
        raise ValueError("Output already exists; refusing to overwrite it")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.output.parent) as temporary:
        work = Path(temporary)
        toolkit = work / "toolkit"
        toolkit.mkdir()
        shutil.copytree(args.bundle, toolkit / "bundle")
        scripts = toolkit / "scripts/appliance"
        scripts.mkdir(parents=True)
        for name in ("manage.py", "run.py", "login.py", "openbox.xml", "install.sh",
                     "verify.sh", "uninstall.sh", "maintenance.sh", "bundle.sh"):
            shutil.copy2(ROOT / "scripts/appliance" / name, scripts / name)
        shutil.copy2(ROOT / "LICENSE", toolkit / "LICENSE")
        shutil.copy2(ROOT / "vm/TOOLKIT_README.md", toolkit / "README.md")
        (toolkit / "PROVENANCE.json").write_text(json.dumps({
            "base_sha256": args.base_sha256.lower(),
            "bundle_manifest_sha256": args.bundle_sha256.lower(),
            "kind": "synthetic-proof-toolkit", "root_filesystem_modified": False,
            "toolkit_files": {name: appliance.digest(path)
                              for name, path in appliance.regular_tree(toolkit).items()},
        }, sort_keys=True, indent=2) + "\n")
        output = work / "preview.iso"
        subprocess.run(["xorriso", "-indev", str(base), "-outdev", str(output),
                        "-boot_image", "any", "replay", "-map", str(toolkit), "/specter-mirror",
                        "-volid", "SPECTERMIRROR", "-commit", "-end"], check=True)
        subprocess.run(["xorriso", "-indev", str(output), "-osirrox", "on",
                        "-extract", "/specter-mirror/bundle/SHA256SUMS", str(work / "manifest")], check=True)
        if appliance.digest(work / "manifest") != args.bundle_sha256.lower():
            raise ValueError("ISO payload verification failed")
        output.rename(args.output)
    print("Created toolkit preview ISO. Live filesystem and official installer are unchanged.")
    print("No guest boot, installed-system test or hardware compatibility is implied.")
    print("ISO SHA-256:", appliance.digest(args.output))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--base-sha256", required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--bundle-sha256", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allow-proof", action="store_true")
    args = parser.parse_args()
    try:
        build(args)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"ISO preparation failed: {error}\n")


if __name__ == "__main__":
    main()
