#!/usr/bin/env python3
"""Extrai imagens, fontes e textos do zip oficial do 8BitDo Ultimate Software V2
para ui/assets/ (fora do git: pertencem à 8BitDo).

Uso: python3 extract_assets.py ~/Downloads/8BitDo_Ultimate_Software_V2_Windows_V1.37.zip
Precisa do ilspycmd (dotnet tool install -g ilspycmd).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
DEST = os.environ.get("U2_ASSETS_DIR") or os.path.join(HERE, "ui", "assets")
APP = "8BitDo Ultimate Software V2"


def ilspy(*args):
    exe = os.environ.get("ILSPYCMD") or shutil.which("ilspycmd") or os.path.expanduser("~/.dotnet/tools/ilspycmd")
    env = dict(os.environ, DOTNET_ROLL_FORWARD="Major")
    subprocess.run([exe, *args], check=True, env=env, stdout=subprocess.DEVNULL)


def strings(cs_path):
    s = open(cs_path, encoding="utf-8-sig").read()
    pat = (r'public static string (\w+)\(\)\s*\{\s*string \w+ = "((?:[^"\\]|\\.)*)";'
           r'\s*if \(IsChineseSimple\(\)\)\s*\{\s*\w+ = "((?:[^"\\]|\\.)*)";')
    return {m[1]: {"en": m[2], "zh": m[3]} for m in re.finditer(pat, s)}


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    with tempfile.TemporaryDirectory() as tmp:
        with zipfile.ZipFile(sys.argv[1]) as z:
            exe = next(n for n in z.namelist() if n.endswith(f"{APP}.exe"))
            z.extract(exe, tmp)
        bundle = os.path.join(tmp, "bundle")
        ilspy("-d", "-o", bundle, os.path.join(tmp, exe))
        src = os.path.join(tmp, "src")
        ilspy("-p", "-o", src, os.path.join(bundle, f"{APP}.dll"))

        if os.path.isdir(DEST):
            shutil.rmtree(DEST)
        n = 0
        for root, _, files in os.walk(src):
            for f in files:
                if f.lower().endswith((".png", ".ttf", ".gif", ".svg", ".ico")):
                    rel = os.path.relpath(os.path.join(root, f), src).lower()
                    out = os.path.join(DEST, rel)
                    os.makedirs(os.path.dirname(out), exist_ok=True)
                    shutil.copyfile(os.path.join(root, f), out)
                    n += 1
        table = strings(os.path.join(src, "AdvanceSuper.Data", "LanguageTools.cs"))
        with open(os.path.join(DEST, "strings.json"), "w") as f:
            json.dump(table, f, ensure_ascii=False)
    print(f"{n} arquivos e {len(table)} textos em {DEST}")


if __name__ == "__main__":
    main()
