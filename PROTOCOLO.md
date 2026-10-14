# Protocolo de configuração — 8BitDo Ultimate 2

Fonte: engenharia reversa, para interoperabilidade, do 8BitDo Ultimate Software V2 (Windows),
conferida no hardware real (Ultimate 2 pelo dongle 2.4G). Nada do app oficial é distribuído aqui.
Trabalho de referência para os modelos antigos (2.4G/BT/PC):
[s8n/ultimatecontroller-rs](https://github.com/s8n/ultimatecontroller-rs).

## Dispositivo

| USB ID      | O que é                                   |
| ----------- | ----------------------------------------- |
| `2dc8:6013` | dongle 2.4G (`PID_Ultimate2RR`), modo DInput |
| `2dc8:310b` | dongle no modo XInput/PC (`modo XInput`)   |
| `2dc8:6012` | controle no cabo (`PID_Ultimate2`)        |

A interface de configuração é a que tem página vendor (`06 xx FF`) com
relatório de saída `0x81` e de entrada `0x02`, 63 bytes cada. No Linux é um
`/dev/hidrawN`; o `uaccess` do systemd já dá acesso ao usuário logado.

O app sempre chama `setGaempadType(0x6012)`, mesmo com o dongle — e isso
decide o formato do pacote abaixo.

## Pacote (host → controle)

Envio no ramo 0x6012 (sem byte de tamanho):

```
off  tam  campo
0    1    0x81            report ID
1    1    0x04            tipo
2    2    cmd   (u16 LE)
4    2    sub   (u16 LE)
6    2    len   (u16 LE)  bytes de dados neste pacote (máx 45)
8    2    crc   (u16 LE)  CRC-16 dos dados (só quando há dados)
10   4    total (u32 LE)
14   4    offset(u32 LE)
18   45   dados
```

Total sempre 64 bytes (zeros no fim).

## Resposta (controle → host)

Resposta: `02 04 <tipo u16> <cmd u16> <len u16> ...`, dados a partir do
byte 18. Leitura: tipo 4, cmd 2. Escrita: tipo 4, cmd 1.

## Comandos

| cmd     | sub     | uso |
| ------- | ------- | --- |
| `7`     | 0 / 1   | `report_state`: 0 pausa o input do controle, 1 retoma. O app envolve **toda** operação com 0…1 |
| `2`     | 0       | leitura: `len`=min(45, restante), `total`, `offset`; repete até completar |
| `1`     | 0       | escrita parcial: `len`, `offset`=posição do campo no struct, `total`=0x638 |
| `6`     | `0x123` | salvar (depois das escritas, em `gravação do nome`) |
| `0x120` | 0       | controle ligado ao dongle? `dados[0]==1` |
| `3`     | `0x123` | `readKeys`: estado atual (12 bytes): botões u32, LX, LY, RX, RY (centro 128), RT, LT. Bits 0x4000/0x8000 = LT/RT no fim do curso. O app pergunta em loop (~500/s possíveis) |
| `8`     | estado  | `setRUMBLE` (teste de vibração da aba Vibration): (R<<24)\|(L<<16)\|1000 — ainda não portado |
| `0x40`  | efeito  | `escrita`: efeito de LED (global): 2 Dynamic, 3 Hotwheel, 4 Custom, 5 Off (6 = Off no modelo MM) |
| `0x41`  | 0       | `leitura do efeito`: `dados[0]` = efeito atual |
| `0x105` | 0       | `getmode_CMD`: `dados[0]` 1 → variante 1; 0 → variante 2 (sem aba Vibration, outra imagem do verso) |
| `0x102` | modo<<8 \| perfil | leitura de macro: lê passos de macro (ver "Passos das macros") |
| `0x103` | modo<<8 \| perfil | escrita de macro: grava passos de macro |
| `0x104` | modo<<8 \| perfil | apagar macro: apaga os 4096 bytes de uma macro |

Cor do controle (`getUltimate2Color`): comando cru `81 05 00 21 01`, resposta
`02 22 …`, cor em `[6]` (2 MM, 3 XT, 4 LT; resto = padrão).

Perfil ativo: ler 20 bytes (`len=total=0x14`, offset 0); `cur_slot` no byte 0x12.

## Cuidados (aprendidos no controle real)

- **`len` nunca passa de 45**, nem na leitura: o app corta em 0x2d. Pedindo
  "o resto" (ex. len=1007 no offset 585), o controle ignora o pacote e reenvia a
  resposta anterior, o que corrompe a leitura sem erro aparente.
- **Conferir o offset da resposta** (bytes 14-17) contra o pedido; descartar se não bater.
- Nunca escrever 0x0C-0x13 (crc, modo, perfil ativo): o app relê `cur_slot` antes.

## CRC-16

o CRC do struct e o CRC dos dados de cada pacote usam tabelas de 2
entradas em 0x10c4e0a8/0x10c4e0ac, ambas `{0x0000, 0xA001}`: **CRC-16/MODBUS**
(refletido, poly 0xA001, init 0xFFFF, sem xor final). O `crc_value` do struct (0x0C) é calculado pelo app sobre o
struct inteiro **sem zerar o campo antes**; no controle testado está `0xFFFFFFFF`
(nunca gravado), então ele não serve para validar uma leitura.

## Struct `struct de configuração` (1592 bytes)

Tamanho confirmado pelo export `CRC do struct_Ultimate2@1592`; offsets por
os offsets do struct.

```
0x000  u32[3]   flag              0x20200911 = perfil gravado; 0xFFFFFFFF = vazio
0x00C  u32      crc_value
0x010  u16      gamepad_mode      0 Switch, 1 DInput, 2 Mac, 3 XInput
0x012  u16      cur_slot          0-2; 3 = sem perfil (padrão)
0x014  32×3     file_name         UTF-16 **big-endian**
0x074  12×3     rumble            flag, float[2] escala
0x098   8×3     joystick          flag, (início,fim)×2  padrão 17..128
0x0B0   8×3     trigger           flag, (início,fim)×2  padrão 0..255 (XInput 77..255)
0x0C8   8×3     special_feature   flag, bits
0x0E0  92×3     map_key           flag, u32[22] (bitmask do botão de destino)
                                  posições: A B X Y L1 R1 L2 R2 L3 R3 Select Start Share Home
                                  Up Down Left Right P1 P2 P3 P4
                                  bits (enum KeyMap / changeKey()): A 0x2000, B 0x1000, X 0x10,
                                  Y 0x20, L1 0x400, R1 0x800, L2 0x4000, **R2 0x8000**, L3 0x2,
                                  R3 0x4, Select 0x8, Start 0x1, **Home 0x20000**, Share 0x10000,
                                  Up 0x200, Down 0x100, Left 0x80, Right 0x40, P1 0x2000000,
                                  P2 0x4000000, P3 0x200000, P4 0x40000000, nenhum 0
                                  (as constantes "originais" do app têm R2/Home trocados; no
                                  Ultimate 2 ele chama changeKey() antes de usar)
0x1F4 216×3     bloco de macros  flag, total, 4× registro de macro (52)
0x47C   8×3     x_rumble          flag, (início,fim)×2
0x494  12×3     sixaxis           flag, keys_map, trigger_mode, sensibilidade, zona morta, tipo
0x4B8  12×3     dynamic (LED)     flag, cor, fundo
0x4DC  16×3     hotwheel (LED)    flag, cor, fundo, velocidade
0x50C 100×3     single (LED)      flag, u32[24] cores
```

Especial (`special_feature`): 0x1/0x2 inverte X/Y esquerdo, 0x4/0x8 X/Y direito,
0x10 troca sticks, 0x80 troca gatilhos, 0x100 direcional ↔ stick esquerdo.
Vibração: `zoom_out` = nível × 0,2 (0–5). Gatilhos 0–255 mostrados como 0–100.

### `bloco de macros` (216 bytes por perfil, em 0x1F4)

```
+0   u32  flag        0x20200911 quando o perfil tem macros
+4   u8   total_cnt   o app sempre grava 4
+8   4× registro de macro (52 bytes):
     +0   32  nome (UTF-16 BE)
     +32  u8  gamepad_mode (o modo do controle)
     +33  u8  special_flag (0)
     +34  u16 max_steps   nº de passos (máx. 200)
     +36  u16 offset      idx × 4096
     +40  u32 key_map     tecla que dispara (bits do KeyMap: A 0x2000 ... P4 0x40000000)
     +44  u32 cycles_num  1 = uma vez; 0xFFFFFFFF = repete; N = N vezes
     +48  u32 interval_ms intervalo entre repetições
```

Só conta como macro (`leitura de macros`/`teclas válidas de macro`) se `key_map` for uma destas teclas:
A, B, X, Y, direcional, L1, L2, L3, R1, R2, R3, Select, Start, Share (0x10000), P1–P4 e P5 (0x100000).
Home não pode ter macro. Ao ativar ou desativar uma macro numa tecla, o app devolve o mapeamento
daquela tecla ao padrão (`resetMapping`).

## Passos das macros (leitura de macro / escrita de macro / apagar macro)

Os passos não ficam no struct de 1592 bytes: cada uma das 4 macros de cada perfil tem uma área de
4096 bytes numa memória à parte, endereçada por `offset = idx × 4096` e pelo `sub`
`(gamepad_mode << 8) | perfil` (perfil 0-2). O app (biblioteca nativa do app oficial, `0x10412800`,
`0x1041a180`, `0x10420170`, desmontados com capstone) usa o mesmo cabeçalho de 64 bytes, mas
o app corta `len` em **32** (e não 45) e passa `offset`/`total` como `u16`.

Passo (`passo`, 10 bytes, LE):

```
+0 u16 ms_tmes        quanto tempo segurar este estado
+2 u16 keys           botões (KeyMap): A 0x2000, B 0x1000, X 0x10, Y 0x20, L1 0x400, R1 0x800,
                      L2 0x4000, R2 0x8000, L3 0x2, R3 0x4, Select 0x8, Start 0x1,
                      cima 0x200, baixo 0x100, esquerda 0x80, direita 0x40
+4 u16 trigger_value  0 no Ultimate 2 (L2/R2 vão em keys; ver abaixo)
+6 u16 left_joy       byte baixo = X, alto = Y: 0x00 esq./cima, 0x7F centro, 0xFF dir./baixo
+8 u16 right_joy      (só 9 valores: 0x7F7F centro, 0x007F cima, 0xFF7F baixo, 0x7F00 esq., 0x7FFF dir.,
                      0x0000, 0x00FF, 0xFF00, 0xFFFF diagonais)
```

`o app` decide entre `keys` e `trigger_value` pelo `gamepad_mode` de outra classe
(`UltimateBTAdvance`), que no Ultimate 2 nunca é lido (fica 0): o app sempre põe L2/R2 em `keys`.

### Ler (`0x102`)

```
81 04 | 02 01 | <sub> | len (≤32) | crc | total = nº de bytes | offset = idx×4096 + já lidos | 32 bytes
```

O app manda `len` bytes de lixo da pilha (`0xCC`, preenchimento do /RTC do MSVC) com o CRC deles;
o u2tool faz igual. Resposta `02 04 04 00 02 01 <len> ...`, dados a partir do byte 18; o app soma o
`len` da resposta e repete até completar (30 tentativas). O u2tool também confere o offset ecoado.

### Apagar (`0x104`) — antes de gravar

```
81 04 | 04 01 | <sub> | len = 0x1000 | crc = 0 | total = 0 | offset = idx×4096 | sem dados
```

(O app põe o tamanho, 4096, no campo `len`/`crc`.) O app espera a resposta `02 04 04 00 04 01`
por até 3 s e não reenvia.

### Gravar (`0x103`)

Os passos (`n × 10` bytes) vão arredondados para múltiplo de 32 (`getCount`), em pacotes de 32:

```
81 04 | 03 01 | <sub> | len = 32 | crc dos 32 bytes | total = idx×4096 + tamanho arredondado | offset = idx×4096 + pos | 32 bytes
```

Note que `total` é o **fim** da área gravada, não o tamanho. O app avança pelo `len` da resposta
(`02 04 04 00 03 01`) e, no fim, manda o `6`/`0x123` de salvar (até 30 vezes, 100 ms entre elas).
O app completa o último pacote com o que houver na memória depois do array; o u2tool completa com
passos neutros (0 ms, nada apertado, sticks no centro). O u2tool relê os passos depois de gravar.

Exemplo — 7 passos na macro 3 (idx 2) do perfil 2, modo XInput (3): `sub` = `0x0301`, 70 bytes →
96, três pacotes com `offset` 0x2000, 0x2020, 0x2040 e `total` = 0x2060.

### Ordem do app (`writeAdvance` + `gravação dos passos`)

1. grava o struct (cmd 1, já com o `bloco de macros` novo) e salva (cmd 6);
2. para cada macro com passos: `0x104` → `0x103`… → cmd 6;
3. tudo dentro de `report_state` 0 … 1.

Ao mover/carregar um perfil o app regrava os passos de cada macro com tecla no perfil de destino.

### Gravação (MacroSetting)

Durante a gravação o app manda `report_state(0)` (o controle para de mandar input ao sistema) e lê
`readKeys` a cada ~10 ms; volta com `report_state(1)` ao parar. Cada mudança vira um passo com o
tempo desde a mudança anterior. Sticks contam como direção ao passar de 67 / 187 (centro 127);
gatilho analógico conta como L2/R2 apertado se for diferente de 0. Máximo de 199 mudanças.

## Fora de escopo

Atualização de firmware, porque pode brickar o controle.
