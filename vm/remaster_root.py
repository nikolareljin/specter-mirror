"""Prepare the appliance filesystem inside an isolated build container."""

import argparse
import configparser
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess


def installer_defaults(root):
    path = root / "etc/minstall.conf"
    configuration = configparser.ConfigParser(interpolation=None)
    configuration.optionxform = str
    if path.exists():
        configuration.read(path)
    if not configuration.has_section("User"):
        configuration.add_section("User")
    configuration.set("User", "Autologin", "true")
    with path.open("w") as stream:
        configuration.write(stream)


def prepare(args):
    root = Path("/work/rootfs")
    bundle = Path("/toolkit/bundle")
    subprocess.run(["unsquashfs", "-no-progress", "-processors", "2", "-d", str(root), "/base/linuxfs"], check=True)
    packages = sorted((bundle / "packages").glob("*.deb"))
    if not packages:
        raise ValueError("Appliance media requires a reviewed offline runtime package set")
    temporary = root / "tmp/specter-runtime-packages"
    if temporary.exists():
        raise ValueError("Base contains an unexpected package staging directory")
    temporary.mkdir()
    devices = []
    try:
        for name, minor in (("null", 3), ("zero", 5), ("random", 8), ("urandom", 9)):
            path = root / "dev" / name
            if not path.exists():
                path.parent.mkdir(exist_ok=True)
                os.mknod(path, stat.S_IFCHR | 0o666, os.makedev(1, minor))
                devices.append(path)
        for path in packages:
            shutil.copy2(path, temporary / path.name)
        subprocess.run(["chroot", str(root), "/usr/bin/dpkg", "--install",
                        *["/tmp/specter-runtime-packages/" + path.name for path in packages]], check=True)
    finally:
        shutil.rmtree(temporary)
        for path in devices:
            path.unlink()
    entries = [line.split(":") for line in (root / "etc/passwd").read_text().splitlines()
               if line.startswith("demo:")]
    if len(entries) != 1 or entries[0][5] != "/home/demo" or int(entries[0][2]) == 0:
        raise ValueError("The base must declare the standard non-root antiX live account")
    home = root / "home/demo"
    if not home.exists():
        shutil.copytree(root / "etc/skel", home, symlinks=True)
        for path in (home, *home.rglob("*")):
            os.chown(path, int(entries[0][2]), int(entries[0][3]), follow_symlinks=False)
    subprocess.run(["python3", "/toolkit/scripts/appliance/manage.py", "install", "--root", str(root),
                    "--bundle", str(bundle), "--sha256", args.sha256, "--user", "demo",
                    "--profile", "generic", "--allow-proof", "--offline", "--follow-default-user"], check=True)
    helper = root / "usr/local/sbin/specter-mirror-live-install"
    helper.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(bundle / "runtime/live-install.sh", helper)
    helper.chmod(0o755)
    policy = root / "etc/sudoers.d/specter-mirror-live"
    policy.write_text("demo ALL=(root) NOPASSWD: /usr/local/sbin/specter-mirror-live-install\n")
    policy.chmod(0o440)
    installer_defaults(root)
    check = subprocess.run(["chroot", str(root), "/usr/bin/ldd", "/opt/specter-mirror/bin/spook-kiosk"],
                           text=True, capture_output=True, check=True)
    if "not found" in check.stdout or "not found" in check.stderr:
        raise ValueError("The prebuilt binary has unresolved runtime libraries in this base")
    for tool in ("openbox", "wmctrl", "xterm", "python3"):
        if not (root / "usr/bin" / tool).exists():
            raise ValueError("Missing kiosk runtime: " + tool)
    subprocess.run(["mksquashfs", str(root), "/work/linuxfs.new", "-noappend", "-no-progress",
                    "-processors", "2", "-comp", "xz"], check=True)
    Path("/work/root-provenance.json").write_text(json.dumps({
        "runtime_packages": [path.name for path in packages],
        "squashfs_tools": subprocess.check_output(["mksquashfs", "-version"], text=True).splitlines()[0],
    }, sort_keys=True, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sha256", required=True)
    args = parser.parse_args()
    if (os.environ.get("SPECTER_ISO_BUILD") != "1" or not Path("/base/linuxfs").is_file()
            or not Path("/toolkit/bundle/SHA256SUMS").is_file() or Path("/work/rootfs").exists()):
        parser.error("Run only in the isolated ISO builder with fresh mounted work storage")
    try:
        prepare(args)
    finally:
        shutil.rmtree("/work/rootfs", ignore_errors=True)


if __name__ == "__main__":
    main()
