#!/usr/bin/env python3
"""Gera o AppImage do u2tool a partir do zip oficial do 8BitDo Ultimate Software V2.

Uso:
    python3 build_appimage.py [ZIP] [--saida DIR] [--assets DIR] [--so-verificar] [--manter-trabalho]

    ZIP            zip oficial (padrão: o mais novo de ~/Downloads/8BitDo_Ultimate_Software_V2*.zip)
    --assets DIR   usa imagens/textos já extraídos (pula dotnet e ilspycmd)
    --saida DIR    onde fica o .AppImage (padrão: ./dist)
    --so-verificar mostra o que seria reaproveitado e o que seria baixado, sem baixar nada

Faz tudo numa pasta de trabalho em ~/.cache e a apaga no fim: só o .AppImage sobra.
Reaproveita o que já existe no sistema (cargo, dotnet, ilspycmd, appimagetool) e baixa
só o que faltar: rustup (static.rust-lang.org), SDK do .NET (dot.net), ilspycmd (nuget.org),
appimagetool (github.com/AppImage) e as crates do projeto (crates.io).
Precisa já instalados: python3, um compilador C (cc), curl ou wget (só se baixar o .NET).

O AppImage leva imagens e textos da 8BitDo extraídos do zip que VOCÊ baixou: uso pessoal,
não redistribua.
"""
import glob
import hashlib
import os
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import urllib.request
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
NAME = "8BitDo_Ultimate_Software_V2-x86_64.AppImage"
ILSPY_VERSION = "11.1.0.9782"
DOTNET_CHANNEL = "10.0"
URL_RUSTUP = "https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init"
URL_DOTNET = "https://dot.net/v1/dotnet-install.sh"
URL_APPIMAGETOOL = "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"

DESKTOP = """[Desktop Entry]
Type=Application
Name=8BitDo Ultimate Software V2
GenericName=Configurador do 8BitDo Ultimate 2
Comment=Perfis, mapeamento, macros, sticks, gatilhos, vibração e RGB do Ultimate 2
Exec=u2tool
Icon=u2tool
Terminal=false
Categories=Game;Utility;Settings;
Keywords=8bitdo;controle;gamepad;ultimate;
StartupWMClass=u2tool
"""

APPRUN = """#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export APPDIR="${APPDIR:-$HERE}"
exec "$HERE/usr/bin/u2tool" "$@"
"""


def say(msg):
    print(f"==> {msg}", flush=True)


def run(cmd, env, **kw):
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True, env=env, **kw)


def fetch(url, dest):
    say(f"baixando {url}")
    with urllib.request.urlopen(url, timeout=60) as r, open(dest, "wb") as f:
        shutil.copyfileobj(r, f)
    print(f"   {os.path.getsize(dest) / 1e6:.1f} MB", flush=True)


def make_exec(path):
    os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def png(w, h, rgba):
    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    rows = b"".join(b"\0" + rgba[y * w * 4:(y + 1) * w * 4] for y in range(h))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def ico_to_png(ico, out):
    """Maior imagem do .ico (PNG embutido ou DIB 32 bpp) para PNG, sem Pillow."""
    d = open(ico, "rb").read()
    n = struct.unpack_from("<H", d, 4)[0]
    best = max(((d[6 + 16 * i] or 256), i) for i in range(n))[1]
    size, off = struct.unpack_from("<II", d, 6 + 16 * best + 8)
    img = d[off:off + size]
    if img[:8] == b"\x89PNG\r\n\x1a\n":
        open(out, "wb").write(img)
        return
    hdr, w, h2, _, bpp, comp = struct.unpack_from("<IiiHHI", img, 0)
    if bpp != 32 or comp != 0:
        sys.exit(f"ícone não suportado (bpp={bpp}, compressão={comp})")
    h = h2 // 2
    px = img[hdr:hdr + w * h * 4]
    rows = [px[(h - 1 - y) * w * 4:(h - y) * w * 4] for y in range(h)]  # de baixo para cima
    rgba = bytearray()
    for row in rows:
        for i in range(0, len(row), 4):
            rgba += bytes((row[i + 2], row[i + 1], row[i], row[i + 3]))
    open(out, "wb").write(png(w, h, bytes(rgba)))


def find_zip():
    zs = glob.glob(os.path.expanduser("~/Downloads/8BitDo_Ultimate_Software_V2*.zip"))
    return max(zs, key=os.path.getmtime) if zs else None


class Tools:
    """Descobre o que já existe; o que falta é baixado para a pasta de trabalho."""

    def __init__(self, work, need_dotnet):
        self.work = work
        self.need_dotnet = need_dotnet
        self.cargo = shutil.which("cargo")
        self.dotnet = shutil.which("dotnet")
        self.ilspy = (shutil.which("ilspycmd") or
                      next((p for p in [os.path.expanduser("~/.dotnet/tools/ilspycmd")] if os.path.exists(p)), None))
        self.appimagetool = shutil.which("appimagetool")

    def plan(self):
        rows = [("cc (compilador C)", shutil.which("cc") or shutil.which("gcc") or shutil.which("clang"), "falta: instale gcc/clang")]
        rows.append(("cargo", self.cargo, "baixa rustup (~300 MB)"))
        if self.need_dotnet:
            rows.append(("dotnet", self.dotnet or self.ilspy, "baixa SDK .NET (~250 MB)"))
            rows.append(("ilspycmd", self.ilspy, f"baixa ilspycmd {ILSPY_VERSION} (nuget)"))
        rows.append(("appimagetool", self.appimagetool, "baixa appimagetool (~25 MB)"))
        return rows

    def env(self):
        w = self.work
        e = dict(os.environ)
        e.update(
            CARGO_HOME=os.path.join(w, "cargo"), RUSTUP_HOME=os.path.join(w, "rustup"),
            CARGO_TARGET_DIR=os.path.join(w, "target"), NUGET_PACKAGES=os.path.join(w, "nuget"),
            DOTNET_CLI_HOME=os.path.join(w, "dotnet-home"), DOTNET_CLI_TELEMETRY_OPTOUT="1",
            DOTNET_NOLOGO="1", DOTNET_SYSTEM_GLOBALIZATION_INVARIANT="1", TMPDIR=os.path.join(w, "tmp"),
            APPIMAGE_EXTRACT_AND_RUN="1", ARCH="x86_64",
        )
        os.makedirs(e["TMPDIR"], exist_ok=True)
        return e

    def ensure(self):
        w = self.work
        env = self.env()
        bins = []
        if not (shutil.which("cc") or shutil.which("gcc") or shutil.which("clang")):
            sys.exit("falta um compilador C (cc/gcc/clang): instale pelo gerenciador de pacotes")
        if not self.cargo:
            self.fetch_rust(env)
            bins.append(os.path.join(w, "cargo", "bin"))
        if self.need_dotnet:
            if not self.ilspy:
                if not self.dotnet:
                    self.fetch_dotnet(env)
                    bins.append(os.path.join(w, "dotnet"))
                    env["DOTNET_ROOT"] = os.path.join(w, "dotnet")
                    env["PATH"] = os.path.join(w, "dotnet") + os.pathsep + env["PATH"]
                run(["dotnet", "tool", "install", "ilspycmd", "--version", ILSPY_VERSION,
                     "--tool-path", os.path.join(w, "tools")], env)
                self.ilspy = os.path.join(w, "tools", "ilspycmd")
        if not self.appimagetool:
            dest = os.path.join(w, "appimagetool.AppImage")
            fetch(URL_APPIMAGETOOL, dest)
            make_exec(dest)
            self.appimagetool = dest
        if bins:
            env["PATH"] = os.pathsep.join(bins + [env["PATH"]])
        if self.ilspy:
            env["ILSPYCMD"] = self.ilspy
        return env

    def fetch_rust(self, env):
        init = os.path.join(self.work, "rustup-init")
        fetch(URL_RUSTUP, init)
        sha = os.path.join(self.work, "rustup-init.sha256")
        fetch(URL_RUSTUP + ".sha256", sha)
        want = open(sha).read().split()[0]
        got = hashlib.sha256(open(init, "rb").read()).hexdigest()
        if want != got:
            sys.exit(f"sha256 do rustup-init não confere ({got} != {want})")
        make_exec(init)
        run([init, "-y", "--no-modify-path", "--profile", "minimal"], env)

    def fetch_dotnet(self, env):
        if not (shutil.which("curl") or shutil.which("wget")):
            sys.exit("o instalador do .NET precisa de curl ou wget")
        script = os.path.join(self.work, "dotnet-install.sh")
        fetch(URL_DOTNET, script)
        run(["bash", script, "--channel", DOTNET_CHANNEL, "--install-dir", os.path.join(self.work, "dotnet"),
             "--no-path"], env)


def build(zip_path, assets_arg, saida, work, tools):
    env = tools.ensure()
    assets = assets_arg
    if not assets:
        assets = os.path.join(work, "assets")
        say("extraindo imagens e textos do zip oficial")
        run([sys.executable, os.path.join(HERE, "extract_assets.py"), zip_path],
            dict(env, U2_ASSETS_DIR=assets, DOTNET_ROLL_FORWARD="Major"))
    if not os.path.isdir(os.path.join(assets, "resource")):
        sys.exit(f"assets inválidos (falta resource/ em {assets})")

    say("compilando o u2tool (cargo build --release)")
    run(["cargo", "build", "--release", "--locked"], env, cwd=os.path.join(HERE, "rust"))

    say("montando o AppDir")
    appdir = os.path.join(work, "AppDir")
    bindir = os.path.join(appdir, "usr", "bin")
    share = os.path.join(appdir, "usr", "share", "u2tool")
    os.makedirs(bindir)
    os.makedirs(share)
    shutil.copy2(os.path.join(work, "target", "release", "u2tool"), bindir)
    shutil.copytree(assets, os.path.join(share, "assets"))
    ico_to_png(os.path.join(assets, "app.ico"), os.path.join(appdir, "u2tool.png"))
    shutil.copy2(os.path.join(appdir, "u2tool.png"), os.path.join(appdir, ".DirIcon"))
    open(os.path.join(appdir, "u2tool.desktop"), "w").write(DESKTOP)
    apprun = os.path.join(appdir, "AppRun")
    open(apprun, "w").write(APPRUN)
    make_exec(apprun)

    say("gerando o AppImage")
    out = os.path.join(work, NAME)
    run([tools.appimagetool, appdir, out], env)
    os.makedirs(saida, exist_ok=True)
    final = os.path.join(saida, NAME)
    shutil.move(out, final)
    return final


def du(path):
    return sum(os.path.getsize(os.path.join(r, f)) for r, _, fs in os.walk(path) for f in fs
               if not os.path.islink(os.path.join(r, f)))


def main():
    args = sys.argv[1:]
    if "-h" in args or "--help" in args:
        sys.exit(__doc__)
    flags = {"--so-verificar", "--manter-trabalho"}
    check, keep = "--so-verificar" in args, "--manter-trabalho" in args
    args = [a for a in args if a not in flags]
    opts = {}
    for k in ("--saida", "--assets"):
        if k in args:
            i = args.index(k)
            if i + 1 >= len(args):
                sys.exit(f"{k} precisa de um valor")
            opts[k] = os.path.expanduser(args[i + 1])
            del args[i:i + 2]
    if len(args) > 1:
        sys.exit(__doc__)
    saida = opts.get("--saida", os.path.join(HERE, "dist"))
    assets = opts.get("--assets")
    zip_path = os.path.expanduser(args[0]) if args else None
    if not assets:
        zip_path = zip_path or find_zip()
        if not zip_path or not os.path.isfile(zip_path):
            sys.exit("zip oficial não encontrado: passe o caminho ou use --assets DIR (ver --help)")

    cache = os.path.expanduser("~/.cache")
    os.makedirs(cache, exist_ok=True)
    work = tempfile.mkdtemp(prefix="u2-appimage-", dir=cache)
    tools = Tools(work, need_dotnet=not assets)
    try:
        if check:
            say(f"zip: {zip_path or '(usando --assets)'}")
            for name, found, todo in tools.plan():
                print(f"   {name:20} {'já tem: ' + found if found else todo}")
            return
        final = build(zip_path, assets, saida, work, tools)
    finally:
        if keep:
            say(f"pasta de trabalho mantida: {work}")
        else:
            freed = du(work)
            shutil.rmtree(work, ignore_errors=True)
            say(f"pasta de trabalho apagada ({freed / 1e6:.0f} MB liberados)")
    say(f"pronto: {final} ({os.path.getsize(final) / 1e6:.0f} MB)")


if __name__ == "__main__":
    main()
