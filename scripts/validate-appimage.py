#!/usr/bin/env python3
"""Validate LWE AppImage metadata without executing the image or AppRun."""

import argparse
import configparser
import mmap
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import sys
import tempfile


class ValidationError(Exception):
    pass


def run_checked(command):
    result = subprocess.run(command, capture_output=True, text=True, errors="replace")
    if result.returncode:
        detail = (result.stderr.strip() or result.stdout.strip())[-4000:]
        raise ValidationError(f"{command[0]} failed (exit {result.returncode}): {detail}")
    return result.stdout.strip()


def elf_end(data):
    """Find the end of the ELF runtime, excluding appended filesystem data."""
    if len(data) < 64 or data[:4] != b"\x7fELF" or data[8:11] != b"AI\x02":
        raise ValidationError("expected a type 2 ELF AppImage (AI\\x02)")
    elf_class, byte_order = data[4:6]
    if elf_class not in (1, 2) or byte_order not in (1, 2):
        raise ValidationError("invalid ELF class or byte order")
    endian = "<" if byte_order == 1 else ">"
    header_format = endian + ("HHIIIIIHHHHHH" if elf_class == 1 else "HHIQQQIHHHHHH")
    header = struct.unpack_from(header_format, data, 16)
    phoff, shoff = header[4:6]
    ehsize, phentsize, phnum, shentsize, shnum = header[7:12]
    end = 16 + struct.calcsize(header_format)
    if ehsize != end:
        raise ValidationError("invalid ELF header size")
    # Extended numbering is unnecessary for the small AppImage runtime.
    if phnum == 0xFFFF or (shoff and not shnum):
        raise ValidationError("unsupported extended ELF header numbering")

    def check_range(offset, size):
        if offset > len(data) or size > len(data) - offset:
            raise ValidationError("ELF runtime range extends beyond the input file")
        return offset + size

    tables = (
        (phoff, phentsize, phnum, "IIIIIIII" if elf_class == 1 else "IIQQQQQQ", True),
        (shoff, shentsize, shnum, "IIIIIIIIII" if elf_class == 1 else "IIQQQQIIQQ", False),
    )
    for offset, entry_size, count, entry_format, program in tables:
        if not count:
            continue
        entry_format = endian + entry_format
        if not offset or entry_size < struct.calcsize(entry_format):
            raise ValidationError("invalid ELF runtime table")
        end = max(end, check_range(offset, entry_size * count))
        for index in range(count):
            entry = struct.unpack_from(entry_format, data, offset + index * entry_size)
            if program:
                file_offset, file_size = (entry[1], entry[4]) if elf_class == 1 else (entry[2], entry[5])
            else:
                if entry[1] == 8:  # SHT_NOBITS occupies no space in the file.
                    continue
                file_offset, file_size = entry[4:6]
            end = max(end, check_range(file_offset, file_size))
    return end


def squashfs_offset(path):
    """Locate a complete v4 SquashFS superblock after the static ELF runtime."""
    if path.stat().st_size < 64:
        raise ValidationError("input is too short to be an AppImage")
    with path.open("rb") as source, mmap.mmap(source.fileno(), 0, access=mmap.ACCESS_READ) as data:
        offset = elf_end(data)
        while (offset := data.find(b"hsqs", offset)) != -1:
            if offset + 96 <= len(data):
                fields = struct.unpack_from("<5I6H8Q", data, offset)
                _, inodes, _, block_size, _, compression, block_log, _, ids, major, minor, _, used, *tables = fields
                valid = (
                    inodes > 0 and ids > 0 and major == 4 and minor == 0
                    and compression in range(1, 7) and 12 <= block_log <= 20
                    and block_size == 1 << block_log and 96 <= used <= len(data) - offset
                    and all(value == 0xFFFFFFFFFFFFFFFF or 96 <= value < used for value in tables)
                    and all(tables[index] != 0xFFFFFFFFFFFFFFFF for index in (0, 2, 3))
                    and tables[2] <= tables[3]  # inode table precedes the directory table
                )
                if valid:
                    return offset
            offset += 4
    raise ValidationError("no valid v4 SquashFS filesystem found after the ELF runtime")


def resolve_metadata(root, name):
    """Resolve only metadata paths, rejecting every absolute/escaping link hop."""
    pending = list(Path(name).parts)
    current = root
    hops = 0
    while pending:
        part = pending.pop(0)
        if part in ("", "."):
            continue
        if part == "..":
            if current == root:
                raise ValidationError(f"{name}: link target escapes the AppDir")
            current = current.parent
            continue
        candidate = current / part
        try:
            mode = candidate.lstat().st_mode
        except OSError as error:
            raise ValidationError(f"{name}: missing or broken target {candidate}: {error}") from error
        if stat.S_ISLNK(mode):
            target = os.readlink(candidate)
            if os.path.isabs(target):
                raise ValidationError(f"{name}: absolute symlink at {candidate.relative_to(root)} -> {target}")
            hops += 1
            if hops > 40:
                raise ValidationError(f"{name}: symlink loop or excessive link depth")
            pending = list(Path(target).parts) + pending
        else:
            current = candidate
            if pending and not stat.S_ISDIR(mode):
                raise ValidationError(f"{name}: target path component is not a directory")
    if not current.is_file():
        raise ValidationError(f"{name}: expected a regular file")
    return current


def require_image(path, allowed, label):
    mime = run_checked(["file", "--brief", "--mime-type", "--", str(path)])
    if mime not in allowed:
        raise ValidationError(f"{label}: expected {' or '.join(sorted(allowed))}, found {mime}")


def validate_appdir(path):
    root = path.resolve(strict=True)
    if not root.is_dir():
        raise ValidationError("AppDir input is not a directory")
    app_run = resolve_metadata(root, "AppRun")
    if not app_run.stat().st_mode & 0o111 or not os.access(app_run, os.X_OK):
        raise ValidationError("AppRun: entry point is not executable")

    desktops = [entry for entry in root.iterdir() if entry.name.endswith(".desktop")]
    if len(desktops) != 1:
        raise ValidationError(f"expected exactly one root .desktop entry, found {len(desktops)}")
    desktop = resolve_metadata(root, desktops[0].name)
    run_checked(["desktop-file-validate", str(desktop)])
    document = configparser.ConfigParser(interpolation=None, delimiters=("=",), comment_prefixes=("#",))
    document.optionxform = str
    try:
        document.read_string(desktop.read_text(encoding="utf-8"))
        icon = document.get("Desktop Entry", "Icon", fallback="").strip()
    except (configparser.Error, UnicodeError) as error:
        raise ValidationError(f"invalid desktop entry: {error}") from error
    if not icon or "/" in icon or icon in (".", ".."):
        raise ValidationError("desktop Icon= must name a root icon")

    names = [icon] if Path(icon).suffix.lower() in (".png", ".svg") else [icon + ".png", icon + ".svg"]
    icons = [name for name in names if os.path.lexists(root / name)]
    if not icons:
        raise ValidationError(f"desktop Icon={icon}: no matching root PNG/SVG icon")
    for name in icons:
        mime = "image/png" if Path(name).suffix.lower() == ".png" else "image/svg+xml"
        require_image(resolve_metadata(root, name), {mime}, name)
    require_image(resolve_metadata(root, ".DirIcon"), {"image/png"}, ".DirIcon")


def validate_appimage(path):
    image = path.resolve(strict=True)
    if not image.is_file():
        raise ValidationError("AppImage input is not a regular file")
    offset = squashfs_offset(image)
    with tempfile.TemporaryDirectory(prefix="lwe-appimage-") as directory:
        root = Path(directory) / "AppDir"
        run_checked(["unsquashfs", "-no-progress", "-offset", str(offset), "-dest", str(root), str(image)])
        validate_appdir(root)
    return offset


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("appimages", nargs="*", type=Path, help="final AppImage artifacts to validate")
    parser.add_argument("--appdir", action="append", default=[], type=Path, metavar="DIR", help="validate an already extracted AppDir")
    args = parser.parse_args(argv)
    if not args.appimages and not args.appdir:
        parser.error("provide at least one AppImage or --appdir DIR")
    dependencies = ["file", "desktop-file-validate"] + (["unsquashfs"] if args.appimages else [])
    missing = [name for name in dependencies if not shutil.which(name)]
    if missing:
        print(f"FAIL: missing required executable(s): {', '.join(missing)}", file=sys.stderr)
        return 1
    failed = False
    for path, appdir in [(path, True) for path in args.appdir] + [(path, False) for path in args.appimages]:
        try:
            offset = None if appdir else validate_appimage(path)
            if appdir:
                validate_appdir(path)
            detail = "" if offset is None else f" (SquashFS offset {offset})"
            print(f"PASS: {path}{detail}")
        except (ValidationError, OSError, RuntimeError) as error:
            print(f"FAIL: {path}: {error}", file=sys.stderr)
            failed = True
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
