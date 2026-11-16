#!/usr/bin/env python3
"""Copies Horizon to a Nintendo Switch over FTP (sphaira's, ftpd or sys-ftpd), as
docs/switch.md lays it out on the SD card:

    sdmc:/switch/horizon/horizon.nro              the game
    sdmc:/switch/horizon/resources/               .local/horizon-client/26.3 (horizon-prepare)
    sdmc:/switch/horizon-smoke/horizon-smoke.nro  the std smoke test, when built

Usage: switch/install-ftp.py HOST[:PORT] [--sd-prefix /sdmc:] [--jobs 4] [--skip-resources]

The SD card is `/sdmc:` on sphaira's server; servers that show the card as `/` take
`--sd-prefix ""`. Files already on the card with the same size are left as they are.
"""

import argparse
import ftplib
import os
import posixpath
import sys
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def connect(host: str, port: int) -> ftplib.FTP:
    ftp = ftplib.FTP()
    ftp.connect(host, port, timeout=60)
    ftp.login()
    ftp.set_pasv(True)
    return ftp


class Remote:
    """One FTP connection with the directories it has made or seen."""

    def __init__(self, host: str, port: int):
        self.host, self.port = host, port
        self.ftp = connect(host, port)
        self.dirs: set[str] = set()
        self.sizes: dict[str, dict[str, int]] = {}

    def reconnect(self) -> None:
        try:
            self.ftp.close()
        except Exception:
            pass
        self.ftp = connect(self.host, self.port)

    def makedirs(self, path: str) -> None:
        if path in self.dirs or path in ("", "/"):
            return
        parent = posixpath.dirname(path)
        if parent != path:
            self.makedirs(parent)
        try:
            self.ftp.mkd(path)
        except ftplib.error_perm:
            pass  # already there (or a mount root such as /sdmc:)
        self.dirs.add(path)

    def listing(self, directory: str) -> dict[str, int]:
        """Names and sizes in `directory` (empty when it can't be listed)."""
        if directory in self.sizes:
            return self.sizes[directory]
        found: dict[str, int] = {}
        try:
            for name, facts in self.ftp.mlsd(directory, facts=["size", "type"]):
                if facts.get("type") == "file" and "size" in facts:
                    found[name] = int(facts["size"])
        except ftplib.all_errors:
            pass
        self.sizes[directory] = found
        return found

    def upload(self, local: Path, remote: str) -> bool:
        """Sends one file unless it is there with the same size; True when sent."""
        directory, name = posixpath.split(remote)
        if self.listing(directory).get(name) == local.stat().st_size:
            return False
        self.makedirs(directory)
        for attempt in range(3):
            try:
                with open(local, "rb") as f:
                    self.ftp.storbinary(f"STOR {remote}", f, blocksize=1 << 16)
                return True
            except ftplib.all_errors as e:
                if attempt == 2:
                    raise RuntimeError(f"{remote}: {e}") from e
                self.reconnect()
        return True


def plan(sd: str, skip_resources: bool) -> list[tuple[Path, str]]:
    files: list[tuple[Path, str]] = []
    nro = ROOT / "target/switch/horizon.nro"
    if not nro.is_file():
        sys.exit("build it first: switch/build.sh")
    files.append((nro, f"{sd}/switch/horizon/horizon.nro"))
    smoke = ROOT / "target/switch/horizon-smoke.nro"
    if smoke.is_file():
        files.append((smoke, f"{sd}/switch/horizon-smoke/horizon-smoke.nro"))
    resources = ROOT / ".local/horizon-client/26.3"
    if skip_resources:
        return files
    if not (resources / "horizon-resources.json").is_file():
        print(f"no prepared resources in {resources}: sending the game only", file=sys.stderr)
        return files
    for dirpath, _dirs, names in os.walk(resources):
        for name in sorted(names):
            local = Path(dirpath) / name
            rel = local.relative_to(resources).as_posix()
            files.append((local, f"{sd}/switch/horizon/resources/{rel}"))
    return files


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("host", help="the Switch's address, HOST[:PORT] (port 5000 by default)")
    parser.add_argument("--sd-prefix", default="/sdmc:", help="the SD card's path on the server (default /sdmc:)")
    parser.add_argument("--jobs", type=int, default=4, help="parallel connections (default 4)")
    parser.add_argument("--skip-resources", action="store_true", help="send only the .nro files")
    args = parser.parse_args()
    host, _, port = args.host.partition(":")
    port = int(port or 5000)
    files = plan(args.sd_prefix.rstrip("/"), args.skip_resources)
    total = sum(local.stat().st_size for local, _ in files)
    print(f"{len(files)} files, {total / 2**20:.0f} MiB -> ftp://{host}:{port}{args.sd_prefix}/switch/")

    # The .nro files first, alone; then the resources spread over the connections by folder,
    # so each connection makes and lists its own folders.
    first = Remote(host, port)
    head = [f for f in files if f[1].endswith(".nro")]
    rest = [f for f in files if not f[1].endswith(".nro")]
    for local, remote in head:
        first.upload(local, remote)
        print(f"  {remote}")
    first.ftp.quit()

    groups: dict[str, list[tuple[Path, str]]] = {}
    for item in rest:
        groups.setdefault(posixpath.dirname(item[1]), []).append(item)
    buckets: list[list[tuple[Path, str]]] = [[] for _ in range(max(1, args.jobs))]
    for i, (_, items) in enumerate(sorted(groups.items(), key=lambda kv: -len(kv[1]))):
        min(buckets, key=len).extend(items)

    lock = threading.Lock()
    done = {"files": 0, "sent": 0, "bytes": 0}
    errors: list[str] = []

    def work(bucket: list[tuple[Path, str]]) -> None:
        if not bucket:
            return
        remote = Remote(host, port)
        for local, path in bucket:
            try:
                sent = remote.upload(local, path)
            except Exception as e:  # keep going; report at the end
                with lock:
                    errors.append(str(e))
                continue
            with lock:
                done["files"] += 1
                done["sent"] += sent
                done["bytes"] += local.stat().st_size
                if done["files"] % 500 == 0:
                    print(f"  {done['files']}/{len(rest)} files, {done['bytes'] / 2**20:.0f} MiB")
        try:
            remote.ftp.quit()
        except ftplib.all_errors:
            pass

    threads = [threading.Thread(target=work, args=(b,)) for b in buckets]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    print(f"resources: {done['files']} files checked, {done['sent']} sent")
    if errors:
        print(f"{len(errors)} errors:", *errors[:20], sep="\n  ", file=sys.stderr)
        sys.exit(1)
    print("done: start the Homebrew Menu holding R on a game, then pick Horizon")


if __name__ == "__main__":
    main()
