#!/usr/bin/env python3
"""Patches rust-src's std for the Nintendo Switch (docs/switch.md), in the build image.
std's `horizon` port is the 3DS's; on the Switch:
  - available_parallelism: `sysconf` (the startup shim's) answers 3, the application's cores;
  - file locks: newlib has no flock and the Switch runs one program at a time, so
    lock/try_lock/unlock succeed (the game locks its log and each open world)."""
import pathlib, re, subprocess, sys

SWITCH = 'all(target_os = "horizon", target_arch = "aarch64")'
std = pathlib.Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip()) / "lib/rustlib/src/rust/library/std/src"


def patch(rel: str, fn):
    path = std / rel
    s = path.read_text()
    out = fn(s)
    if out != s:
        path.write_text(out)
        print(f"patched {path}")


def parallelism(s: str) -> str:
    if SWITCH in s:
        return s
    old = '''            target_os = "redox",
            target_os = "wasi",
        ) => {
            #[allow(unused_assignments)]'''
    if s.count(old) != 1:
        sys.exit("patch-std: available_parallelism not found as expected")
    return s.replace(old, f'''            target_os = "redox",
            target_os = "wasi",
            // The Nintendo Switch (Horizon's port): sysconf answers 3.
            {SWITCH},
        ) => {{
            #[allow(unused_assignments)]''')


def locks(s: str) -> str:
    start = s.index("    pub fn lock(&self) -> io::Result<()> {")
    end = s.index("\n    }\n", s.index("    pub fn unlock(&self) -> io::Result<()> {")) + len("\n    }\n")
    region = s[start:end]
    if SWITCH in region:
        return s
    arm = (f"\n            // The Nintendo Switch (Horizon's port): no flock, one program at a time.\n"
           f"            {SWITCH} => Ok(()),\n            _ => Err(")
    new, n = re.subn(r"\n            _ => Err\(", arm, region)
    if n != 5:
        sys.exit(f"patch-std: expected 5 file lock fallbacks, found {n}")
    return s[:start] + new + s[end:]


patch("sys/thread/unix.rs", parallelism)
patch("sys/fs/unix.rs", locks)
