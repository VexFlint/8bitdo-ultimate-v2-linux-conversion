# Validação no hardware

Tudo abaixo foi feito em 13/10/2026 num 8BitDo Ultimate 2 pelo dongle 2.4G (`2dc8:310b`,
`2dc8:6013` quando o controle está desligado), em CachyOS. As imagens são saídas reais de
leitura, geradas a partir dos dados lidos do controle.

## Macros

`docs/img/macros-gravar-ler-apagar.png`: dump da configuração depois de (1) gravar uma macro
pelo app, (2) regravar outra e (3) apagar. Os registros de macro (nome, nº de passos, tecla de
disparo) refletem cada etapa; a macro gravada disparou ao apertar a tecla.

## Reconexão

`docs/img/dongle-desligar-religar.png`: captura ao desligar e religar o controle. O nó hidraw
do dongle **some** e volta como `2dc8:6013` (controle desligado, `conectado=False`, último
quadro de teclas congelado); ao ligar, some de novo e volta como `2dc8:310b`. O app reabre o
nó sozinho; antes ficava preso no descritor antigo.

## Estado sem controle

`docs/img/info-controle-desligado.png`: `u2tool.py info` com o controle desligado.

## Não validado

- Outros controles e outras distros (regra udev fora do systemd/`uaccess`).
- Modo cabo (`2dc8:6012`) e modo DInput na gravação de macros.
- Firmware: removido de propósito.
