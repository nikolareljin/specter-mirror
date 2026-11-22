"""Build, inspect and provision a verified SpecterMirror proof bundle."""

import argparse
import ctypes
import fcntl
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
DISPLAY_MANAGERS = {
    "slim": ("etc/slim.conf", "login_cmd", "%session", "auto_login", "yes", "no"),
    "slimski": ("etc/slimski.local.conf", "atlogin_cmd", "%sessiontype", "autologin_enabled", "true", "false"),
}
# antiX removes these live-only shutdown commands during disk installation.
LIVE_POWER_HOOKS = {
    "systemhalt_cmd /usr/local/bin/persist-config --shutdown --command halt",
    "reboot_cmd /usr/local/bin/persist-config --shutdown --command reboot",
}


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
        if not re.fullmatch(r"[A-Za-z0-9_./+:%-]+", name):
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
        match = re.fullmatch(r"([a-f0-9]{64})  ([A-Za-z0-9_./+:%-]+)", line)
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
                 "runtime/openbox.xml", "runtime/live-install.sh", "LICENSE"):
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
        asset_manifest = SOURCE / "assets/manifest.json"
        assets = json.loads(asset_manifest.read_text())
        if assets.get("format") != 1 or len(set(assets["files"])) != len(assets["files"]):
            raise ValueError("Invalid asset manifest")
        (stage / "assets").mkdir()
        shutil.copy2(asset_manifest, stage / "assets/manifest.json")
        for name in assets["files"]:
            source = safe_path(SOURCE / "assets", Path(name))
            target = stage / "assets" / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        shutil.copy2(SOURCE / "LICENSE", stage / "LICENSE")
        (stage / "runtime").mkdir()
        for name in ("manage.py", "run.py", "login.py", "openbox.xml", "live-install.sh"):
            shutil.copy2(Path(__file__).with_name(name), stage / "runtime" / name)
        if args.runtime_packages is not None:
            archives = regular_tree(args.runtime_packages)
            if not archives or any(not name.endswith(".deb") or "/" in name for name in archives):
                raise ValueError("Runtime package directory must contain only reviewed .deb archives")
            (stage / "packages").mkdir()
            for name, path in archives.items():
                architecture = subprocess.check_output(["dpkg-deb", "-f", str(path), "Architecture"], text=True).strip()
                if architecture not in ("i386", "all"):
                    raise ValueError("Runtime packages must target i386 or all")
                shutil.copy2(path, stage / "packages" / name)
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


def fact_path(root, relative):
    path = (root / relative).resolve(strict=True)
    if not path.is_relative_to(root.resolve()):
        raise ValueError("System fact resolves outside the target root")
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


def exchange_directories(first, second):
    library = ctypes.CDLL(None, use_errno=True)
    if not hasattr(library, "renameat2"):
        raise ValueError("Atomic updates require Linux renameat2 support")
    exchange = library.renameat2
    exchange.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    exchange.restype = ctypes.c_int
    if exchange(-100, os.fsencode(first), -100, os.fsencode(second), 2) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code))


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


def kiosk_login(original, user, autologin, manager="slim"):
    _, login_key, token, auto_key, enabled, disabled = DISPLAY_MANAGERS[manager]
    lines = original.splitlines(keepends=True)
    active = [" ".join(line.split()) for line in lines if re.match(r"^\s*" + login_key + r"\s+", line)]
    replacement = f"{login_key} setsid /usr/bin/python3 /opt/specter-mirror/runtime/login.py {token}"
    stock = f"{login_key} setsid /usr/local/bin/desktop-session {token}"
    if active not in ([stock], [replacement]):
        raise ValueError("Unsupported display manager login command; refusing custom login replacement")
    for key in (login_key, auto_key, "default_user"):
        lines = [line for line in lines if not re.match(r"^\s*" + key + r"\s+", line)]
    content = "".join(lines)
    if content and not content.endswith("\n"):
        content += "\n"
    return content + replacement + f"\n{auto_key} {enabled if autologin else disabled}\ndefault_user {user}\n"


def login_fingerprint(text, follow_default_user):
    if follow_default_user:
        text = re.sub(r"^\s*default_user\s+[^\n]*\n?", "", text, flags=re.M)
        text = "".join(line for line in text.splitlines(keepends=True)
                       if " ".join(line.split()) not in LIVE_POWER_HOOKS)
    return hashlib.sha256(text.encode()).hexdigest()


def configured_user(root, config):
    if not config.get("follow_default_user", False):
        return config["user"]
    manager = config["display_manager"]
    text = safe_path(root, Path(DISPLAY_MANAGERS[manager][0])).read_text()
    names = re.findall(r"^\s*default_user\s+([a-z_][a-z0-9_-]*)\s*$", text, re.M)
    if len(names) != 1 or names[0] == "root":
        raise ValueError("Display manager must select exactly one non-root default user")
    return names[0]


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
    archives = sorted((args.bundle / "packages").glob("*.deb"))
    if archives:
        subprocess.run(["dpkg", "--install", *[str(path.resolve()) for path in archives]], check=True)
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
    os_release = fact_path(root, Path("etc/os-release"))
    marker = safe_path(root, Path("etc/antix-version"))
    antix = marker.is_file() and re.match(r"^antiX-[0-9]", marker.read_text())
    if not antix and not re.search(r'^ID=[\"\']?anti[xX][\"\']?$', os_release.read_text(), re.M):
        raise ValueError("Target must identify as antiX through its release marker")
    home, uid, gid = account(root, args.user)
    if not safe_path(root, home).is_dir():
        raise ValueError("The kiosk account's home directory is missing")
    destination = safe_path(root, APP)
    config = safe_path(root, Path("etc/specter-mirror/config.json"))
    startup = safe_path(root, home / ".desktop-session/startup")
    original = startup.read_text() if startup.exists() else ""
    updated = startup_text(original, args.session == "desktop")
    manager = "slimski" if safe_path(root, Path(DISPLAY_MANAGERS["slimski"][0])).is_file() else "slim"
    slim = safe_path(root, Path(DISPLAY_MANAGERS[manager][0]))
    backup = safe_path(root, Path("etc/specter-mirror/display-manager.before"))
    installed_slim = safe_path(root, Path("etc/specter-mirror/display-manager.installed"))
    previous_slim = slim.read_text() if args.session == "kiosk" else None
    next_slim = kiosk_login(previous_slim, args.user, not args.no_autologin, manager) if previous_slim is not None else None
    old_config = json.loads(config.read_text()) if config.exists() else None
    if config.exists() and (not isinstance(old_config, dict) or not old_config.get("user")):
        raise ValueError("Existing kiosk configuration is invalid; refusing replacement")
    if old_config and configured_user(root, old_config) != args.user:
        raise ValueError("Uninstall before changing the kiosk account")
    if old_config and old_config.get("session", "desktop") != args.session:
        raise ValueError("Uninstall before changing the session mode")
    if installed_slim.exists() and old_config is None:
        raise ValueError("Incomplete kiosk configuration; inspect the retained display manager backup")
    if installed_slim.exists() and login_fingerprint(previous_slim, old_config.get("follow_default_user", False)) != installed_slim.read_text().strip():
        raise ValueError("Display manager configuration changed after provisioning; reconcile it before reinstalling")
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
                              "session": args.session, "display_manager": manager,
                              "follow_default_user": args.follow_default_user}
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
        previous = stage if destination.exists() else None
        if previous is not None:
            exchange_directories(stage, destination)
        else:
            stage.rename(destination)
        try:
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
                write(installed_slim, login_fingerprint(next_slim, settings.get("follow_default_user", False)) + "\n")
        except BaseException:
            if previous is not None:
                exchange_directories(previous, destination)
            elif destination.exists():
                shutil.rmtree(destination)
            for path, saved in reversed(changed):
                if saved is None:
                    path.unlink(missing_ok=True)
                else:
                    text, stat = saved
                    atomic_text(path, text, stat.st_mode & 0o777)
                    if root == Path("/"):
                        os.chown(path, stat.st_uid, stat.st_gid)
            raise
    print("Installed the verified Rust display application and session configuration.")
    if root != Path("/"):
        print("Staged only; target runtime and reboot behavior are unverified.")


def verify(args):
    destination = safe_path(args.root, APP)
    check_bundle(destination, args.sha256)
    config = json.loads(safe_path(args.root, Path("etc/specter-mirror/config.json")).read_text())
    home, _, _ = account(args.root, configured_user(args.root, config))
    if config.get("session") == "kiosk":
        manager = config.get("display_manager", "slim")
        slim = safe_path(args.root, Path(DISPLAY_MANAGERS[manager][0])).read_text()
        expected = safe_path(args.root, Path("etc/specter-mirror/display-manager.installed")).read_text()
        if login_fingerprint(slim, config.get("follow_default_user", False)) != expected.strip() or "/opt/specter-mirror/runtime/login.py" not in slim:
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
    user = configured_user(args.root, config)
    home, uid, gid = account(args.root, user)
    startup = safe_path(args.root, home / ".desktop-session/startup")
    updated = startup_text(startup.read_text(), False) if startup.exists() else None
    manager = config.get("display_manager", "slim")
    slim = safe_path(args.root, Path(DISPLAY_MANAGERS[manager][0]))
    backup = safe_path(args.root, Path("etc/specter-mirror/display-manager.before"))
    installed_slim = safe_path(args.root, Path("etc/specter-mirror/display-manager.installed"))
    if config.get("session") == "kiosk":
        current_slim = slim.read_text()
        if login_fingerprint(current_slim, config.get("follow_default_user", False)) != installed_slim.read_text().strip():
            raise ValueError("Display manager configuration changed; refusing to overwrite operator changes")
        previous_slim = backup.read_text()
        if config.get("follow_default_user", False):
            previous_slim = re.sub(r"^\s*default_user\s+[^\n]*", "default_user " + user, previous_slim, flags=re.M)
            current_lines = {" ".join(line.split()) for line in current_slim.splitlines()}
            previous_slim = "".join(line for line in previous_slim.splitlines(keepends=True)
                                    if " ".join(line.split()) not in LIVE_POWER_HOOKS - current_lines)
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
    create.add_argument("--runtime-packages", type=Path, help="Reviewed i386/all .deb archives for offline provisioning")
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
            command.add_argument("--follow-default-user", action="store_true",
                                 help="For generic install media: follow the display manager's selected account")
    args = parser.parse_args()
    descriptor = None
    try:
        if hasattr(args, "root"):
            if not args.root.is_absolute() or ".." in args.root.parts:
                raise ValueError("--root must be an absolute staging directory without traversal")
            safe_path(args.root, Path("."))
            descriptor = os.open(args.root, os.O_RDONLY | os.O_DIRECTORY)
            try:
                fcntl.flock(descriptor, (fcntl.LOCK_SH if args.command == "verify" else fcntl.LOCK_EX) | fcntl.LOCK_NB)
            except BlockingIOError:
                raise ValueError("Another provisioning operation is active for this target") from None
        {"bundle": build, "install": install, "verify": verify, "uninstall": uninstall}[args.command](args)
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Provisioning failed: {error}\n")
    finally:
        if descriptor is not None:
            os.close(descriptor)


if __name__ == "__main__":
    main()
