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
    if not base.is_file():
        raise ValueError("Base ISO must be a regular file")
    if appliance.digest(base) != args.base_sha256.lower():
        raise ValueError("Base ISO checksum mismatch")
    report = subprocess.run(["xorriso", "-indev", str(base), "-report_el_torito", "plain"],
                            capture_output=True, text=True, check=True)
    if not re.search(r"El Torito boot img", report.stdout + report.stderr):
        raise ValueError("Base ISO has no bootable El Torito image")
    appliance.check_bundle(args.bundle, args.bundle_sha256)
    if not args.allow_proof:
        raise ValueError("Only synthetic-proof media is supported; --allow-proof is required")
    if args.output.exists() or args.output.is_symlink():
        raise ValueError("Output already exists; refusing to overwrite it")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/:@-]*", args.builder_image):
        raise ValueError("Builder image must be an explicit image reference")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.output.parent) as temporary:
        work = Path(temporary)
        toolkit = work / "toolkit"
        toolkit.mkdir()
        shutil.copytree(args.bundle, toolkit / "bundle")
        appliance.check_bundle(toolkit / "bundle", args.bundle_sha256)
        scripts = toolkit / "scripts/appliance"
        scripts.mkdir(parents=True)
        for name in ("manage.py", "run.py", "login.py", "openbox.xml", "install.sh",
                     "verify.sh", "uninstall.sh", "maintenance.sh", "bundle.sh", "live-install.sh"):
            shutil.copy2(ROOT / "scripts/appliance" / name, scripts / name)
        shutil.copy2(ROOT / "LICENSE", toolkit / "LICENSE")
        shutil.copy2(ROOT / "vm/TOOLKIT_README.md", toolkit / "README.md")
        modifications = []
        root_provenance = None
        if args.appliance:
            if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.:/@-]*", args.builder_image):
                raise ValueError("Invalid builder image reference")
            if not (args.bundle / "packages").is_dir():
                raise ValueError("Appliance ISO requires a bundle containing offline runtime packages")
            base_root = work / "base-linuxfs"
            subprocess.run(["xorriso", "-indev", str(base), "-osirrox", "on",
                            "-extract", "/antiX/linuxfs", str(base_root)], check=True)
            shutil.copy2(ROOT / "vm/remaster_root.py", toolkit / "remaster_root.py")
            root_work = work / "root-build"
            root_work.mkdir()
            for path in (base_root, toolkit, root_work):
                if "," in str(path):
                    raise ValueError("Container mount paths must not contain commas")
            subprocess.run(["docker", "run", "--rm", "--user", "0:0", "--network", "none", "--env", "SPECTER_ISO_BUILD=1",
                            "--mount", f"type=bind,src={base_root},dst=/base/linuxfs,readonly",
                            "--mount", f"type=bind,src={toolkit},dst=/toolkit,readonly",
                            "--mount", f"type=bind,src={root_work},dst=/work",
                            args.builder_image, "python3", "/toolkit/remaster_root.py",
                            "--sha256", args.bundle_sha256], check=True)
            new_root = root_work / "linuxfs.new"
            # The antiX installer requires this integrity file; SHA-256 pins establish trust.
            checksum = subprocess.check_output(["md5sum", str(new_root)], text=True).split()[0]
            legacy_manifest = work / "linuxfs.md5"
            legacy_manifest.write_text(checksum + "  linuxfs\n")
            modifications = ["-map", str(new_root), "/antiX/linuxfs",
                             "-map", str(legacy_manifest), "/antiX/linuxfs.md5"]
            root_provenance = json.loads((root_work / "root-provenance.json").read_text())
            root_provenance["builder_image_id"] = subprocess.check_output(
                ["docker", "image", "inspect", "--format", "{{.Id}}", args.builder_image], text=True).strip()
        (toolkit / "PROVENANCE.json").write_text(json.dumps({
            "base_sha256": args.base_sha256.lower(),
            "bundle_manifest_sha256": args.bundle_sha256.lower(),
            "kind": "appliance-preview" if args.appliance else "synthetic-proof-toolkit",
            "root_filesystem_modified": args.appliance, "root_build": root_provenance,
            "toolkit_files": {name: appliance.digest(path)
                              for name, path in appliance.regular_tree(toolkit).items()},
        }, sort_keys=True, indent=2) + "\n")
        output = work / "preview.iso"
        subprocess.run(["xorriso", "-indev", str(base), "-outdev", str(output),
                        "-boot_image", "any", "replay", "-map", str(toolkit), "/specter-mirror",
                        *modifications, "-volid", "SPECTERMIRROR", "-commit", "-end"], check=True)
        subprocess.run(["xorriso", "-indev", str(output), "-osirrox", "on",
                        "-extract", "/specter-mirror/bundle", str(work / "verified-bundle")], check=True)
        appliance.check_bundle(work / "verified-bundle", args.bundle_sha256)
        output.rename(args.output)
    print("Created appliance preview ISO." if args.appliance else "Created toolkit preview ISO; live filesystem unchanged.")
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
    parser.add_argument("--appliance", action="store_true", help="Embed the app, runtime packages and kiosk setup in the live filesystem")
    parser.add_argument("--builder-image", default="specter-mirror-iso-builder:local")
    args = parser.parse_args()
    try:
        build(args)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"ISO preparation failed: {error}\n")


if __name__ == "__main__":
    main()
