#!/usr/bin/env python3
"""Configurador Linux para o 8BitDo Ultimate 2 (dongle 2.4G 2dc8:6013 / cabo 2dc8:6012).

Protocolo obtido por engenharia reversa do 8BitDo Ultimate Software V2 1.37
(protocolo obtido por engenharia reversa do app oficial; ver PROTOCOLO.md).
Ver PROTOCOLO.md.

Escrita: só campos alterados, seguida de salvar (cmd 6/0x123) e releitura.
"""
import argparse
import glob
import json
import os
import select
import struct
import sys
import time

VID = 0x2DC8
PIDS = {0x6012: "Ultimate 2 (cabo)", 0x6013: "Ultimate 2 (dongle, modo DInput)",
        0x310B: "Ultimate 2 (dongle, modo XInput/PC)"}

CFG_SIZE = 0x638  # 1592, = sizeof(struct de configuração)
CHUNK = 45        # o app limita a 0x2d bytes por pacote

CMD_WRITE = 1
CMD_READ = 2
CMD_REPORT_STATE = 7   # sub=0 pausa os relatórios de input, sub=1 retoma
CMD_CONNECT = 0x120    # getUltimate2Connect: data[0]==1 -> controle ligado ao dongle

GAMEPAD_MODE = {0: "Switch", 1: "DInput/Android", 2: "macOS", 3: "XInput"}  # enum GAMEPAD_MODE
ENABLE = 0x20200911  # flag de perfil gravado


# ---------------------------------------------------------------- transporte

def find_hidraw():
    """Qualquer interface 2dc8 com página vendor e relatório de saída 0x81.
    O dongle aparece como 6013 (modo DInput/config) ou 310b (modo XInput/PC);
    o cabo como 6012."""
    for dev in sorted(glob.glob("/sys/class/hidraw/hidraw*")):
        try:
            uevent = open(os.path.join(dev, "device/uevent")).read()
            rd = open(os.path.join(dev, "device/report_descriptor"), "rb").read()
        except OSError:
            continue
        hid_id = next((l[7:] for l in uevent.splitlines() if l.startswith("HID_ID=")), None)
        if not hid_id:
            continue
        _, vid, pid = (int(x, 16) for x in hid_id.split(":"))
        if vid == VID and rd[:1] == b"\x06" and rd[2:3] == b"\xff" and b"\x85\x81" in rd:
            return "/dev/" + os.path.basename(dev), pid
    return None, None


class Dev:
    def __init__(self, path):
        self.fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)

    def close(self):
        os.close(self.fd)

    def drain(self):
        while select.select([self.fd], [], [], 0)[0]:
            try:
                os.read(self.fd, 64)
            except BlockingIOError:
                break

    def send(self, cmd, sub=0, length=0, total=0, offset=0, data=b""):
        """Envio, ramo PID 0x6012 (o app usa 0x6012 também com o dongle):
        81 04 <cmd u16> <sub u16> <len u16> <crc u16> <total u32> <offset u32> <dados>"""
        crc = crc16(data) if data else 0
        pkt = struct.pack("<BBHHHHII", 0x81, 0x04, cmd, sub, length, crc, total, offset) + data
        os.write(self.fd, pkt.ljust(64, b"\0"))

    def recv(self, want_type, want_cmd, timeout=0.5, offset=None):
        """Espera 02 04 <type u16> <cmd u16> <len u16> <crc u16> <total u32>
        <offset u32>; dados no byte 18. Com `offset`, descarta respostas de outro offset:
        o controle às vezes reenvia a resposta do pacote anterior."""
        end = time.monotonic() + timeout
        while (left := end - time.monotonic()) > 0:
            if not select.select([self.fd], [], [], left)[0]:
                break
            r = os.read(self.fd, 64)
            if len(r) >= 18 and r[0] == 0x02 and r[1] == 0x04:
                typ, cmd, length = struct.unpack_from("<HHH", r, 2)
                if typ == want_type and cmd == want_cmd and (
                        offset is None or struct.unpack_from("<I", r, 14)[0] == offset):
                    return r, length
        return None, 0

    def recv_cmd(self, want_cmd, timeout=0.5):
        """Resposta 02 04 com o comando ecoado nos bytes 4-5, qualquer tipo."""
        end = time.monotonic() + timeout
        while (left := end - time.monotonic()) > 0:
            if not select.select([self.fd], [], [], left)[0]:
                break
            r = os.read(self.fd, 64)
            if len(r) >= 19 and r[0] == 0x02 and r[1] == 0x04 and struct.unpack_from("<H", r, 4)[0] == want_cmd:
                return r
        return None

    def raw(self, payload, ok, timeout=0.5):
        """Comando "direto" (formato com byte de tamanho), ex. getUltimate2Color."""
        os.write(self.fd, bytes(payload).ljust(64, b"\0"))
        end = time.monotonic() + timeout
        while (left := end - time.monotonic()) > 0:
            if not select.select([self.fd], [], [], left)[0]:
                break
            r = os.read(self.fd, 64)
            if ok(r):
                return r
        return None

    def report_state(self, on):
        self.send(CMD_REPORT_STATE, 1 if on else 0)
        time.sleep(0.03)


def crc16(data):
    """CRC dos pacotes e do struct: tabela {0x0000, 0xA001} em
    0x10c4e0a8/0x10c4e0ac → CRC-16/MODBUS: refletido, poly 0xA001, init 0xFFFF, sem xor final."""
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


# ---------------------------------------------------------------- operações

def is_connected(d):
    for _ in range(5):
        d.send(CMD_CONNECT)
        r, _ = d.recv(4, CMD_CONNECT)
        if r:
            return r[18] == 1
    return None


CMD_THEME_WRITE = 0x40   # escrita: sub = efeito
CMD_THEME_READ = 0x41    # leitura do efeito: dados[0] = efeito
CMD_VARIANT = 0x105      # comando de modo (81 04 05 01)
THEMES = {2: "dynamic", 3: "hotwheel", 4: "custom", 5: "off", 6: "off"}


def read_theme(d):
    """Efeito de LED ativo (global, não é por perfil). 2 Dynamic, 3 Hotwheel, 4 Custom, 5/6 Off."""
    for _ in range(5):
        d.send(CMD_THEME_READ)
        r, _ = d.recv(4, CMD_THEME_READ)
        if r:
            return r[18]
    return None


def write_theme(d, theme):
    for _ in range(5):
        d.send(CMD_THEME_WRITE, theme)
        r, _ = d.recv(4, CMD_THEME_WRITE)
        if r:
            return True
    raise IOError("o controle não confirmou o efeito de LED")


def read_variant(d):
    """S_Const.m_mode: 1 ou 2 (decide a imagem do verso e se a aba Vibration aparece)."""
    for _ in range(5):
        d.send(CMD_VARIANT)
        r = d.recv_cmd(CMD_VARIANT)
        if r:
            return 1 if r[18] == 1 else 2
    return None


def read_color(d):
    """S_Const.m_ControllerColor (0/1 padrão, 2 MM, 3 XT, 4 LT): getUltimate2Color."""
    r = d.raw([0x81, 0x05, 0x00, 0x21, 0x01], lambda r: len(r) > 6 and r[0] == 0x02 and r[1] == 0x22)
    return r[6] if r else 0


CMD_KEYS = 3          # leitura de teclas: cmd 3, sub 0x123 → resposta (4, 3) com 12 bytes
KEYS_SUB = 0x123


def read_keys(d):
    """Estado atual: botões (u32), LX, LY, RX, RY (0-255, centro 128), RT, LT (0-255).
    Os bits 0x4000/0x8000 dos botões indicam LT/RT no fim do curso.
    Esvazia a fila antes e fica com a resposta mais nova: o controle às vezes responde
    atrasado, e pegar a mais velha acumula atraso."""
    d.drain()
    d.send(CMD_KEYS, KEYS_SUB)
    r, n = d.recv(4, CMD_KEYS, timeout=0.05)
    if not r:
        return None
    while select.select([d.fd], [], [], 0)[0]:
        try:
            x = os.read(d.fd, 64)
        except BlockingIOError:
            break
        if len(x) >= 18 and x[0] == 0x02 and x[1] == 0x04 and struct.unpack_from("<HH", x, 2) == (4, CMD_KEYS):
            r, n = x, struct.unpack_from("<H", x, 6)[0]
    if n < 10:
        return None
    k = r[18:18 + n]
    return {"b": struct.unpack_from("<I", k, 0)[0], "lx": k[4], "ly": k[5], "rx": k[6],
            "ry": k[7], "rt": k[8], "lt": k[9]}


CMD_RUMBLE = 8


def rumble(d, left, right, ms=1000):
    """vibração: cmd 8, (R<<24)|(L<<16)|ms): cmd 8, sub = duração em ms e as
    intensidades (0-255) nos bytes do campo len. Não grava nada, só aciona os motores."""
    d.send(CMD_RUMBLE, ms, (left & 0xFF) | (right & 0xFF) << 8)
    d.recv_cmd(CMD_RUMBLE, timeout=0.1)


def read_config(d, size=CFG_SIZE):
    """leitura do struct: pede (len=min(45, restante), offset, total); a resposta traz até 45 bytes."""
    buf = bytearray()
    tries = 0
    while len(buf) < size:
        off = len(buf)
        # o app limita o len do cabeçalho a 0x2d: pedir "o resto" faz o controle
        # ignorar alguns pacotes e repetir a resposta anterior.
        d.send(CMD_READ, 0, min(CHUNK, size - off), size, off)
        r, n = d.recv(4, CMD_READ, offset=off)
        if not r or n == 0:
            tries += 1
            if tries >= 30:
                raise IOError(f"sem resposta lendo offset {off}")
            continue
        tries = 0
        buf += r[18:18 + n]
    return bytes(buf[:size])


CMD_SAVE = 6
SAVE_SUB = 0x123


def write_range(d, off, data):
    """escrita) devolve quantos bytes entraram."""
    pos = 0
    tries = 0
    while pos < len(data):
        chunk = data[pos:pos + CHUNK]
        d.send(CMD_WRITE, 0, len(chunk), CFG_SIZE, off + pos, chunk)
        r, n = d.recv(4, CMD_WRITE, offset=off + pos)
        if not r:
            tries += 1
            if tries >= 30:
                raise IOError(f"sem confirmação escrevendo offset {off + pos}")
            continue
        tries = 0
        pos += n or len(chunk)


def save(d):
    """Depois das escritas: Sleep(100); cmd 6 sub 0x123; espera (4,6)."""
    time.sleep(0.1)
    d.send(CMD_SAVE, SAVE_SUB)
    r, _ = d.recv(4, CMD_SAVE, timeout=4)
    if not r:
        raise IOError("o controle não confirmou o salvamento")


# Campos na granularidade em que o app os grava (offset, tamanho). flag[] vai de 4 em 4
# (gravação do nome grava só o do perfil) e 0x0C-0x13 (crc, modo, perfil ativo) nunca é
# escrito: o app relê cur_slot do controle antes de qualquer escrita.
def fields():
    f = [(0x000, 4), (0x004, 4), (0x008, 4)]
    for base, size in [(0x014, 32), (0x074, 12), (0x098, 8), (0x0B0, 8), (0x0C8, 8),
                       (0x0E0, 92), (0x1F4, 216), (0x47C, 8), (0x494, 12), (0x4B8, 12),
                       (0x4DC, 16), (0x50C, 100)]:
        f += [(base + size * i, size) for i in range(3)]
    return f


def diff_fields(old, new):
    return [(o, n) for o, n in fields() if old[o:o + n] != new[o:o + n]]


def read_range(d, off, n):
    """Leitura parcial: mesmo pacote do loop de leitura do struct (len, offset, total=0x638)."""
    buf = bytearray()
    tries = 0
    while len(buf) < n:
        d.send(CMD_READ, 0, min(CHUNK, n - len(buf)), CFG_SIZE, off + len(buf))
        r, got = d.recv(4, CMD_READ, offset=off + len(buf))
        if not r or got == 0:
            tries += 1
            if tries >= 30:
                raise IOError(f"sem resposta lendo offset {off + len(buf)}")
            continue
        tries = 0
        buf += r[18:18 + min(got, n - len(buf))]
    return bytes(buf)


def apply_changes(d, base, new):
    """Grava em `d` só os campos em que `new` difere de `base` (o que a interface mudou),
    salva e relê exatamente esses trechos para conferir. Devolve (campos, cur_slot)."""
    changed = diff_fields(base, new)
    for o, n in changed:
        write_range(d, o, new[o:o + n])
    if changed:
        save(d)
        bad = [o for o, n in changed if read_range(d, o, n) != new[o:o + n]]
        if bad:
            raise IOError("releitura não bate nos campos " + ", ".join(hex(o) for o in bad))
    slot = struct.unpack_from("<H", read_range(d, 0, 0x14), 0x12)[0]
    return changed, slot


def apply_config(d, new):
    """restore: compara com o que está no controle agora."""
    return apply_changes(d, read_config(d), new)[0]


# ---------------------------------------------------------------- decodificação
# Layout = offsets de struct de configuração (total 1592, confere com
# o sufixo @1592 de CRC do struct_Ultimate2 na DLL).

OFF = dict(flag=0x000, crc=0x00C, mode=0x010, slot=0x012, name=0x014, rumble=0x074,
           joy=0x098, trig=0x0B0, special=0x0C8, mapkey=0x0E0, macro=0x1F4,
           xrumble=0x47C, sixaxis=0x494, dynamic=0x4B8, hotwheel=0x4DC, single=0x50C)


def decode(b):
    u32 = lambda o: struct.unpack_from("<I", b, o)[0]
    out = {
        "crc_value": f"0x{u32(OFF['crc']):08x}",
        "gamepad_mode": GAMEPAD_MODE.get(struct.unpack_from("<H", b, OFF["mode"])[0],
                                         struct.unpack_from("<H", b, OFF["mode"])[0]),
        "cur_slot": struct.unpack_from("<H", b, OFF["slot"])[0],  # 0-2; 3 = sem perfil
        "perfis": [],
    }
    for i in range(3):
        raw = b[OFF["name"] + 32 * i: OFF["name"] + 32 * (i + 1)]
        # S_File.stringToBytes: UTF-16 big-endian, preenchido com zeros
        name = raw.decode("utf-16-be", "replace").split("\0")[0] if raw[:2] != b"\xff\xff" else ""
        rb = OFF["rumble"] + 12 * i
        jb = OFF["joy"] + 8 * i
        tb = OFF["trig"] + 8 * i
        sb = OFF["special"] + 8 * i
        mb = OFF["mapkey"] + 92 * i
        xb = OFF["xrumble"] + 8 * i
        gb = OFF["sixaxis"] + 12 * i
        db = OFF["dynamic"] + 12 * i
        hb = OFF["hotwheel"] + 16 * i
        lb = OFF["single"] + 100 * i
        mc = OFF["macro"] + 216 * i
        out["perfis"].append({
            "gravado": u32(4 * i) == ENABLE,
            "nome": name,
            "vibracao": {"flag": f"0x{u32(rb):x}",
                         "escala": list(struct.unpack_from("<2f", b, rb + 4))},
            "sticks": {"flag": f"0x{u32(jb):x}",
                       "esq": list(b[jb + 4:jb + 6]), "dir": list(b[jb + 6:jb + 8])},
            "gatilhos": {"flag": f"0x{u32(tb):x}",
                         "esq": list(b[tb + 4:tb + 6]), "dir": list(b[tb + 6:tb + 8])},
            "especial": {"flag": f"0x{u32(sb):x}", "bits": f"0x{u32(sb + 4):08x}"},
            "mapa": {"flag": f"0x{u32(mb):x}",
                     "teclas": [f"0x{k:08x}" for k in struct.unpack_from("<22I", b, mb + 4)]},
            "macros": {"flag": f"0x{u32(mc):x}", "total": b[mc + 4]},
            "vibracao_xinput": {"flag": f"0x{u32(xb):x}",
                                "esq": list(b[xb + 4:xb + 6]), "dir": list(b[xb + 6:xb + 8])},
            "giroscopio": {"flag": f"0x{u32(gb):x}", "teclas": f"0x{u32(gb + 4):08x}",
                           "modo_gatilho": b[gb + 8], "sensibilidade": b[gb + 9],
                           "zona_morta": b[gb + 10], "tipo": b[gb + 11]},
            "led_dinamico": {"flag": f"0x{u32(db):x}", "cor": f"#{u32(db + 4):06x}",
                             "fundo": f"#{u32(db + 8):06x}"},
            "led_hotwheel": {"flag": f"0x{u32(hb):x}", "cor": f"#{u32(hb + 4):06x}",
                             "fundo": f"#{u32(hb + 8):06x}", "velocidade": b[hb + 12]},
            "led_fixo": {"flag": f"0x{u32(lb):x}",
                         "cores": [f"#{c:06x}" for c in struct.unpack_from("<24I", b, lb + 4)]},
        })
    return out


# ---------------------------------------------------------------- CLI

def with_device(fn):
    path, pid = find_hidraw()
    if not path:
        sys.exit("Ultimate 2 não encontrado (nenhuma interface de config 2dc8).")
    print(f"{PIDS.get(pid, hex(pid))} em {path}", file=sys.stderr)
    d = Dev(path)
    try:
        d.drain()
        d.report_state(False)
        d.drain()
        return fn(d)
    finally:
        try:
            d.report_state(True)  # sempre devolve o input normal do controle
        finally:
            d.close()


def cmd_info(_):
    def run(d):
        c = is_connected(d)
        print("controle ligado ao dongle:", {True: "sim", False: "não", None: "sem resposta"}[c])
        if c:
            head = read_config(d, 0x14)
            slot = struct.unpack_from("<H", head, 0x12)[0]
            print("modo:", GAMEPAD_MODE.get(struct.unpack_from("<H", head, 0x10)[0]),
                  "| perfil ativo:", slot + 1 if slot < 3 else "nenhum (padrão)")
    with_device(run)


def cmd_dump(a):
    def run(d):
        cfg = read_config(d)
        with open(a.saida, "wb") as f:
            f.write(cfg)
        dec = decode(cfg)
        print(json.dumps(dec, indent=1, ensure_ascii=False))
        stored = struct.unpack_from("<I", cfg, 0x0C)[0]
        z = bytearray(cfg); z[0x0C:0x10] = b"\0\0\0\0"
        print(f"\nsalvo em {a.saida} ({len(cfg)} bytes)", file=sys.stderr)
        print(f"crc guardado 0x{stored:04x} | crc(tudo) 0x{crc16(cfg):04x} | "
              f"crc(campo zerado) 0x{crc16(z):04x}", file=sys.stderr)
    with_device(run)


def cmd_restore(a):
    new = open(a.arquivo, "rb").read()
    if len(new) != CFG_SIZE:
        sys.exit(f"tamanho errado: {len(new)} (esperado {CFG_SIZE})")
    if not a.sim:
        sys.exit("escrita no controle: confirme com --sim")
    def run(d):
        changed = apply_config(d, new)
        print(f"{len(changed)} campo(s) gravado(s) e conferido(s):",
              ", ".join(hex(o) for o, _ in changed) or "nada mudou")
    with_device(run)


def cmd_show(a):
    print(json.dumps(decode(open(a.arquivo, "rb").read()), indent=1, ensure_ascii=False))


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    s = p.add_subparsers(required=True)
    s.add_parser("info", help="conexão, modo e perfil ativo").set_defaults(fn=cmd_info)
    d = s.add_parser("dump", help="lê a configuração inteira e salva um backup")
    d.add_argument("saida", nargs="?", default=time.strftime("u2-backup-%Y%m%d-%H%M%S.bin"))
    d.set_defaults(fn=cmd_dump)
    v = s.add_parser("show", help="decodifica um backup salvo")
    v.add_argument("arquivo")
    v.set_defaults(fn=cmd_show)
    r = s.add_parser("restore", help="grava um arquivo .bin no controle (só os campos que mudaram)")
    r.add_argument("arquivo")
    r.add_argument("--sim", action="store_true", help="confirma a escrita")
    r.set_defaults(fn=cmd_restore)
    a = p.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
