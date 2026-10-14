#!/bin/sh
# Instala o u2tool (Rust) como app do usuário: binário, imagens, ícone e atalho no menu.
#   ./install.sh            instala/atualiza
#   ./install.sh --remover  desinstala (mantém os perfis em ~/.local/share/u2tool/perfis)
set -e
cd "$(dirname "$0")"
BIN="$HOME/.local/bin/u2tool"
SHARE="$HOME/.local/share/u2tool"
ICON="$HOME/.local/share/icons/hicolor/512x512/apps/u2tool.png"
DESK="$HOME/.local/share/applications/u2tool.desktop"

if [ "$1" = "--remover" ]; then
    rm -rf "$BIN" "$SHARE/assets" "$ICON" "$DESK"
    update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
    echo "removido"
    exit 0
fi

[ -d ui/assets/resource ] || { echo "faltam as imagens: rode extract_assets.py"; exit 1; }
(cd rust && cargo build --release)
mkdir -p "$(dirname "$BIN")" "$SHARE" "$(dirname "$ICON")" "$(dirname "$DESK")"
install -m755 rust/target/release/u2tool "$BIN"
rm -rf "$SHARE/assets"
cp -r ui/assets "$SHARE/assets"
python3 -c "from PIL import Image; Image.open('ui/assets/app.ico').save('$ICON')"
cat > "$DESK" <<DESKTOP
[Desktop Entry]
Type=Application
Name=8BitDo Ultimate Software V2
GenericName=Configurador do 8BitDo Ultimate 2
Comment=Perfis, mapeamento, sticks, gatilhos, vibração e RGB do Ultimate 2
Exec=$BIN
Icon=u2tool
Terminal=false
Categories=Game;Utility;Settings;
Keywords=8bitdo;controle;gamepad;ultimate;
StartupWMClass=u2tool
DESKTOP
update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
gtk-update-icon-cache -q "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
echo "instalado: $BIN (atalho no menu: 8BitDo Ultimate Software V2)"
