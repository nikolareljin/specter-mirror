"""Run a disposable antiX guest without physical disks or shared host directories."""

import argparse
import fcntl
import hashlib
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", type=Path, required=True)
    parser.add_argument("--iso-sha256", required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--memory", type=int, default=1024)
    parser.add_argument("--accel", choices=("auto", "kvm", "tcg"), default="auto")
    parser.add_argument("--network", choices=("none", "user"), default="none")
    parser.add_argument("--headless", action="store_true")
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    if not 512 <= args.memory <= 4096:
        parser.error("Memory must be between 512 and 4096 MB")
    descriptor = None
    try:
        iso = args.iso.resolve(strict=True)
        result = hashlib.sha256()
        with iso.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                result.update(block)
        if result.hexdigest() != args.iso_sha256.lower():
            parser.error("ISO does not match the trusted checksum")
        directory = args.directory.absolute()
        if any(path.is_symlink() for path in (directory, *directory.parents)):
            parser.error("VM directory must not use symbolic links")
        disk = directory / "appliance.qcow2"
        if args.resume:
            if not disk.is_file() or disk.is_symlink():
                parser.error("No regular appliance.qcow2 exists for --resume")
        else:
            if directory.exists():
                parser.error("Choose a new disposable directory, or use --resume")
            directory.mkdir(parents=True)
        descriptor = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            parser.error("This VM directory already has an active controller")
        if not args.resume:
            subprocess.run(["qemu-img", "create", "-f", "qcow2", str(disk), "16G"], check=True)
        accel = args.accel
        if accel == "auto":
            accel = "kvm" if os.access("/dev/kvm", os.R_OK | os.W_OK) else "tcg"
        command = ["qemu-system-i386", "-name", "specter-mirror-validation", "-accel", accel,
                   "-m", str(args.memory), "-smp", "1", "-vga", "std",
                   "-drive", "file=" + str(disk).replace(",", ",,") + ",format=qcow2",
                   "-cdrom", str(iso), "-boot", "c" if args.resume else "once=d"]
        if args.network == "none":
            command += ["-nic", "none"]
        else:
            command += ["-netdev", "user,id=net0", "-device", "e1000,netdev=net0"]
        if args.headless:
            command += ["-display", "none", "-monitor",
                        "unix:" + str(directory / "monitor.sock").replace(",", ",,") + ",server=on,wait=off",
                        "-serial", "file:" + str(directory / "serial.log").replace(",", ",,")]
        subprocess.run(command, check=True, pass_fds=(descriptor,))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"QEMU validation failed: {error}\n")
    except KeyboardInterrupt:
        parser.exit(130, "VM interrupted; the virtual disk is retained.\n")
    finally:
        if descriptor is not None:
            os.close(descriptor)


if __name__ == "__main__":
    main()
