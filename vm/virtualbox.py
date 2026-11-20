"""Create a new disposable BIOS VM without modifying existing machines."""

import argparse
import hashlib
from pathlib import Path
import re
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", required=True)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--iso-sha256", required=True)
    parser.add_argument("--directory", required=True, type=Path)
    parser.add_argument("--memory", type=int, default=1024)
    parser.add_argument("--disk-mb", type=int, default=16384)
    args = parser.parse_args()
    if (not re.fullmatch(r"specter-mirror-[a-z0-9-]+", args.name)
            or not 512 <= args.memory <= 4096 or not 8192 <= args.disk_mb <= 32768):
        parser.error("Use a specter-mirror-NAME, 512-4096 MB RAM and an 8192-32768 MB disk")
    iso = args.iso.resolve(strict=True)
    checksum = hashlib.sha256()
    with iso.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(block)
    if checksum.hexdigest() != args.iso_sha256.lower():
        parser.error("ISO does not match the trusted --iso-sha256")
    directory = args.directory.resolve()
    if directory.exists():
        parser.error("VM directory already exists; choose a new disposable directory")
    machines = subprocess.check_output(["VBoxManage", "list", "vms"], text=True)
    if any(line.startswith(f'"{args.name}" ') for line in machines.splitlines()):
        parser.error("A VM with this name already exists; it will not be changed")
    directory.mkdir(parents=True)
    disk = directory / "appliance.vdi"
    commands = [
        ["createvm", "--name", args.name, "--ostype", "Debian", "--basefolder", str(directory), "--register"],
        ["modifyvm", args.name, "--memory", str(args.memory), "--cpus", "1", "--firmware", "bios",
         "--graphicscontroller", "vboxvga", "--accelerate-3d", "off", "--vram", "16",
         "--nic1", "nat", "--clipboard-mode", "disabled", "--drag-and-drop", "disabled",
         "--boot1", "dvd", "--boot2", "disk"],
        ["createmedium", "disk", "--filename", str(disk), "--size", str(args.disk_mb), "--format", "VDI"],
        ["storagectl", args.name, "--name", "IDE", "--add", "ide", "--controller", "PIIX4"],
        ["storageattach", args.name, "--storagectl", "IDE", "--port", "0", "--device", "0",
         "--type", "hdd", "--medium", str(disk)],
        ["storageattach", args.name, "--storagectl", "IDE", "--port", "1", "--device", "0",
         "--type", "dvddrive", "--medium", str(iso)],
    ]
    try:
        for command in commands:
            subprocess.run(["VBoxManage", *command], check=True)
    except subprocess.SubprocessError:
        parser.exit(1, "VM creation stopped. Partial files are retained for inspection; no existing VM was removed.\n")
    print("VM created but not started. Install antiX interactively; do not automate disk erasure.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(f"VM preparation failed: {error}")
