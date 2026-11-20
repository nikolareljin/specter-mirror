"""Build, inspect and provision a verified SpecterMirror proof bundle."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import sys
import tempfile


SOURCE = Path(__file__).resolve().parents[2]
BEGIN = "# BEGIN SpecterMirror managed startup"
END = "# END SpecterMirror managed startup"
APP = Path("opt/specter-mirror")
PACKAGES = ["libx11-6", "libxext6", "libxcursor1", "libxrandr2", "libxi6", "wmctrl", "openbox", "xterm"]
SLIM_LOGIN = "login_cmd setsid /usr/bin/python3 /opt/specter-mirror/runtime/login.py %session"


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def regular_tree(root):
    if root.is_symlink() or not root.is_dir():
        raise ValueError("Bundle must be a real directory")
    files = {}
    for path in sorted(root.rglob("*")):
        name = path.relative_to(root).as_posix()
        if path.is_symlink() or not (path.is_file() or path.is_dir()):
            raise ValueError("Bundle contains a link or special file")
        if not re.fullmatch(r"[A-Za-z0-9_./-]+", name):
            raise ValueError("Bundle path contains unsupported characters")
        if path.is_file():
            files[name] = path
    return files


def check_elf(path):
    with path.open("rb") as stream:
        header = stream.read(52)
    if (len(header) != 52 or header[:7] != b"\x7fELF\x01\x01\x01"
            or struct.unpack_from("<H", header, 16)[0] not in (2, 3)
            or struct.unpack_from("<H", header, 18)[0] != 3):
        raise ValueError("Expected a little-endian ELF32 x86 executable")


def check_bundle(bundle, expected):
    if not re.fullmatch(r"[a-fA-F0-9]{64}", expected):
        raise ValueError("--sha256 must be the trusted SHA-256 of SHA256SUMS")
    files = regular_tree(bundle)
    manifest = files.get("SHA256SUMS")
    if manifest is None or digest(manifest) != expected.lower():
        raise ValueError("Bundle manifest does not match the trusted checksum")
    recorded = {}
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r"([a-f0-9]{64})  ([A-Za-z0-9_./-]+)", line)
        if not match or match[2] in recorded or match[2] == "SHA256SUMS":
            raise ValueError("Invalid or duplicate bundle manifest entry")
        recorded[match[2]] = match[1]
    if set(recorded) != set(files) - {"SHA256SUMS"}:
        raise ValueError("Bundle has missing or unlisted files")
    for name, checksum in recorded.items():
        if digest(files[name]) != checksum:
            raise ValueError("Bundle file checksum mismatch")
    metadata = json.loads(files["bundle.json"].read_text())
    if metadata != {"format": 1, "kind": "synthetic-proof", "target": "i686-unknown-linux-gnu"}:
        raise ValueError("Unsupported bundle format or application kind")
    for name in ("bin/spook-kiosk", "runtime/manage.py", "runtime/run.py", "runtime/login.py",
                 "runtime/openbox.xml", "LICENSE"):
        if name not in files:
            raise ValueError("Bundle is missing a required runtime file")
    check_elf(files["bin/spook-kiosk"])
    return metadata


def build(args):
    check_elf(args.binary)
    if args.output.exists():
        raise ValueError("Output already exists; choose a new bundle directory")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.output.parent) as temporary:
        stage = Path(temporary) / "bundle"
        (stage / "bin").mkdir(parents=True)
        shutil.copy2(args.binary, stage / "bin/spook-kiosk")
        (stage / "bin/spook-kiosk").chmod(0o755)
        shutil.copytree(SOURCE / "assets", stage / "assets")
        shutil.copy2(SOURCE / "LICENSE", stage / "LICENSE")
        (stage / "runtime").mkdir()
        for name in ("manage.py", "run.py", "login.py", "openbox.xml"):
            shutil.copy2(Path(__file__).with_name(name), stage / "runtime" / name)
        (stage / "bundle.json").write_text(json.dumps({
            "format": 1, "kind": "synthetic-proof", "target": "i686-unknown-linux-gnu",
        }, sort_keys=True) + "\n")
        paths = regular_tree(stage)
        (stage / "SHA256SUMS").write_text("".join(
            f"{digest(path)}  {name}\n" for name, path in paths.items()
        ))
        check_bundle(stage, digest(stage / "SHA256SUMS"))
        stage.rename(args.output)
    print("Trusted manifest SHA-256:", digest(args.output / "SHA256SUMS"))


def safe_path(root, relative):
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError("Managed path must remain below the installation root")
    path = root / relative
    for parent in (path, *path.parents):
        if parent.is_symlink():
            raise ValueError("Managed path contains a symbolic link")
    return path


def atomic_text(path, content, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            stream.write(content)
            os.fchmod(stream.fileno(), mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            Path(temporary).unlink()


def account(root, name):
    if not re.fullmatch(r"[a-z_][a-z0-9_-]*", name) or name == "root":
        raise ValueError("Select an existing non-root kiosk account")
    rows = (root / "etc/passwd").read_text().splitlines()
    for row in rows:
        values = row.split(":")
        if len(values) == 7 and values[0] == name:
            home = Path(values[5])
            if (int(values[2]) == 0 or not home.is_absolute()
                    or not home.is_relative_to("/home") or ".." in home.parts
                    or len(home.parts) < 3):
                raise ValueError("Kiosk account must have its own home under /home")
            return home.relative_to("/"), int(values[2]), int(values[3])
    raise ValueError("Selected kiosk account does not exist")


def startup_text(original, enable):
    if original.count(BEGIN) != original.count(END) or original.count(BEGIN) > 1:
        raise ValueError("Malformed managed startup block; repair it before provisioning")
    if BEGIN in original:
        start, finish = original.index(BEGIN), original.index(END) + len(END)
        if finish < start:
            raise ValueError("Malformed managed startup block")
        if original[finish:finish + 1] == "\n":
            finish += 1
        original = original[:start] + original[finish:]
    if not enable:
        return original
    if original and not original.endswith("\n"):
        original += "\n"
    return original + BEGIN + "\npython3 /opt/specter-mirror/runtime/run.py &\n" + END + "\n"


def kiosk_login(original, user, autologin):
    lines = original.splitlines(keepends=True)
    active = [line.strip() for line in lines if re.match(r"^\s*login_cmd\s+", line)]
    if active not in (["login_cmd setsid /usr/local/bin/desktop-session %session"], [SLIM_LOGIN]):
        raise ValueError("Unsupported SLiM login_cmd; refusing to replace custom login behavior")
    for key in ("login_cmd", "auto_login", "default_user"):
        lines = [line for line in lines if not re.match(r"^\s*" + key + r"\s+", line)]
    content = "".join(lines)
    if content and not content.endswith("\n"):
        content += "\n"
    return content + SLIM_LOGIN + f"\nauto_login {'yes' if autologin else 'no'}\ndefault_user {user}\n"


def runtime_check(args):
    root = args.root
    if root != Path("/"):
        return
    if os.geteuid() != 0:
        raise ValueError("System provisioning requires root; do not run the kiosk as root")
    if (platform.machine() not in ("i386", "i486", "i586", "i686", "x86_64")
            or subprocess.check_output(["getconf", "LONG_BIT"], text=True).strip() != "32"):
        raise ValueError("Install into 32-bit antiX userspace, not the development host")
    if not re.search(r"\bsse2\b", Path("/proc/cpuinfo").read_text()):
        raise ValueError("This standard Rust i686 build requires SSE2")
    if args.install_deps:
        subprocess.run(["apt-get", "update"], check=True)
        subprocess.run(["apt-get", "install", "-y", "--no-install-recommends", *PACKAGES], check=True)
    tools = ["wmctrl"] + (["openbox", "xterm"] if args.session == "kiosk" else [])
    if any(not shutil.which(tool) for tool in tools):
        raise ValueError("A session runtime dependency is missing; install runtime packages first")
    check = subprocess.run(["ldd", str(args.bundle / "bin/spook-kiosk")],
                           text=True, capture_output=True, timeout=30)
    if check.returncode or "not found" in check.stdout or "not found" in check.stderr:
        raise ValueError("Binary runtime libraries or glibc are incompatible; inspect ldd locally")


def install(args):
    check_bundle(args.bundle, args.sha256)
    if not args.allow_proof:
        raise ValueError("Synthetic proof only; --allow-proof is required")
    if args.offline and args.install_deps:
        raise ValueError("--offline cannot be combined with --install-deps")
    root = args.root
    if root.is_symlink() or not root.is_absolute() or ".." in root.parts:
        raise ValueError("--root must be an absolute, non-symlink staging directory")
    os_release = safe_path(root, Path("etc/os-release"))
    if not re.search(r'^ID=[\"\']?anti[xX][\"\']?$', os_release.read_text(), re.M):
        raise ValueError("Target must identify as antiX in /etc/os-release")
    home, uid, gid = account(root, args.user)
    if not safe_path(root, home).is_dir():
        raise ValueError("The kiosk account's home directory is missing")
    destination = safe_path(root, APP)
    config = safe_path(root, Path("etc/specter-mirror/config.json"))
    startup = safe_path(root, home / ".desktop-session/startup")
    original = startup.read_text() if startup.exists() else ""
    updated = startup_text(original, args.session == "desktop")
    slim = safe_path(root, Path("etc/slim.conf"))
    backup = safe_path(root, Path("etc/specter-mirror/slim.conf.before"))
    installed_slim = safe_path(root, Path("etc/specter-mirror/slim.conf.installed"))
    previous_slim = slim.read_text() if args.session == "kiosk" else None
    next_slim = kiosk_login(previous_slim, args.user, not args.no_autologin) if previous_slim is not None else None
    old_config = json.loads(config.read_text()) if config.exists() else None
    if old_config and old_config.get("user") != args.user:
        raise ValueError("Uninstall before changing the kiosk account")
    if old_config and old_config.get("session", "desktop") != args.session:
        raise ValueError("Uninstall before changing the session mode")
    if installed_slim.exists() and previous_slim != installed_slim.read_text():
        raise ValueError("SLiM configuration changed after provisioning; reconcile it before reinstalling")
    if destination.exists() and not (destination / "SHA256SUMS").is_file():
        raise ValueError("Existing application directory is unmanaged; refusing replacement")
    if destination.exists():
        check_bundle(destination, digest(destination / "SHA256SUMS"))
    runtime_check(args)
    destination.parent.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(destination.parent).free < sum(p.stat().st_size for p in regular_tree(args.bundle).values()) * 2:
        raise ValueError("Insufficient space to stage the application")
    profile = args.profile
    if profile == "auto":
        model_file = root / "sys/class/dmi/id/product_name"
        model = model_file.read_text().lower() if model_file.exists() else ""
        profile = "nb205" if "nb205" in model else "d630" if "d630" in model else "generic"
    settings = old_config or {"user": args.user, "profile": profile, "mode": "synthetic-proof",
                              "session": args.session}
    changed = []

    def write(path, text, mode=0o644):
        previous = (path.read_text(), path.stat()) if path.exists() else None
        atomic_text(path, text, mode)
        changed.append((path, previous))

    with tempfile.TemporaryDirectory(dir=destination.parent) as temporary:
        stage = Path(temporary) / "next"
        shutil.copytree(args.bundle, stage)
        for path in stage.rglob("*"):
            path.chmod(0o755 if path.is_dir() else 0o644)
        stage.chmod(0o755)
        (stage / "bin/spook-kiosk").chmod(0o755)
        check_bundle(stage, args.sha256)
        previous = Path(temporary) / "previous"
        if destination.exists():
            destination.rename(previous)
        try:
            stage.rename(destination)
            if not config.exists():
                write(config, json.dumps(settings, sort_keys=True) + "\n")
            if args.session == "desktop" or BEGIN in original:
                write(startup, updated, 0o755)
                if root == Path("/"):
                    os.chown(startup.parent, uid, gid)
                    os.chown(startup, uid, gid)
            if next_slim is not None:
                if not backup.exists():
                    write(backup, previous_slim, 0o600)
                write(slim, next_slim)
                write(installed_slim, next_slim, 0o600)
        except BaseException:
            if destination.exists():
                shutil.rmtree(destination)
            if previous.exists():
                previous.rename(destination)
            for path, saved in reversed(changed):
                if saved is None:
                    path.unlink(missing_ok=True)
                else:
                    text, stat = saved
                    atomic_text(path, text, stat.st_mode & 0o777)
                    if root == Path("/"):
                        os.chown(path, stat.st_uid, stat.st_gid)
            raise
    print("Installed synthetic proof. Camera and effect support are pending.")
    if root != Path("/"):
        print("Staged only; target runtime and reboot behavior are unverified.")


def verify(args):
    destination = safe_path(args.root, APP)
    check_bundle(destination, args.sha256)
    config = json.loads(safe_path(args.root, Path("etc/specter-mirror/config.json")).read_text())
    home, _, _ = account(args.root, config["user"])
    if config.get("session") == "kiosk":
        slim = safe_path(args.root, Path("etc/slim.conf")).read_text()
        expected = safe_path(args.root, Path("etc/specter-mirror/slim.conf.installed")).read_text()
        if slim != expected or SLIM_LOGIN not in slim:
            raise ValueError("Dedicated kiosk login configuration changed")
    else:
        startup = safe_path(args.root, home / ".desktop-session/startup").read_text()
        if BEGIN not in startup or startup_text(startup, True) != startup:
            raise ValueError("Managed startup block is missing or malformed")
    print("Installed files and session configuration verified; no display or hardware test performed.")


def uninstall(args):
    destination = safe_path(args.root, APP)
    check_bundle(destination, args.sha256)
    config_path = safe_path(args.root, Path("etc/specter-mirror/config.json"))
    config = json.loads(config_path.read_text())
    home, uid, gid = account(args.root, config["user"])
    startup = safe_path(args.root, home / ".desktop-session/startup")
    updated = startup_text(startup.read_text(), False) if startup.exists() else None
    slim = safe_path(args.root, Path("etc/slim.conf"))
    backup = safe_path(args.root, Path("etc/specter-mirror/slim.conf.before"))
    installed_slim = safe_path(args.root, Path("etc/specter-mirror/slim.conf.installed"))
    if config.get("session") == "kiosk":
        if slim.read_text() != installed_slim.read_text():
            raise ValueError("SLiM configuration changed; refusing to overwrite operator changes")
        previous_slim = backup.read_text()
    if args.root == Path("/") and os.geteuid() != 0:
        raise ValueError("System uninstall requires root")
    if config.get("session") == "kiosk":
        atomic_text(slim, previous_slim)
        backup.unlink()
        installed_slim.unlink()
    elif updated is not None:
        atomic_text(startup, updated, 0o755)
        if args.root == Path("/"):
            os.chown(startup, uid, gid)
    shutil.rmtree(destination)
    config_path.unlink()
    print("Removed managed application and startup block; user state and unrelated startup commands remain.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("bundle", help="Create an opt-in synthetic-proof bundle")
    create.add_argument("--binary", type=Path, required=True)
    create.add_argument("--output", type=Path, required=True)
    for name in ("install", "verify", "uninstall"):
        command = commands.add_parser(name)
        command.add_argument("--root", type=Path, default=Path("/"), help="Stage files only when not /")
        command.add_argument("--sha256", required=True, help="Trusted SHA-256 of bundle SHA256SUMS")
        if name == "install":
            command.add_argument("--bundle", type=Path, required=True)
            command.add_argument("--user", required=True)
            command.add_argument("--profile", choices=("auto", "nb205", "d630", "virtualbox", "generic"), default="auto")
            command.add_argument("--allow-proof", action="store_true")
            command.add_argument("--offline", action="store_true")
            command.add_argument("--install-deps", action="store_true")
            command.add_argument("--session", choices=("kiosk", "desktop"), default="kiosk")
            command.add_argument("--no-autologin", action="store_true")
    args = parser.parse_args()
    try:
        if hasattr(args, "root"):
            if not args.root.is_absolute() or ".." in args.root.parts:
                raise ValueError("--root must be an absolute staging directory without traversal")
            safe_path(args.root, Path("."))
        {"bundle": build, "install": install, "verify": verify, "uninstall": uninstall}[args.command](args)
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Provisioning failed: {error}\n")


if __name__ == "__main__":
    main()
