"""Comprueba que la paleta del panel cumple los contrastes minimos.

Se escribio porque la paleta original tenia el borde de una celda vacia a 1,29:1
sobre el panel: sobre fondo negro no se apreciaba donde estaba cada boton. Los
colores se eligieron despejando el contraste deseado, y este script evita que una
edicion futura los baje sin darse cuenta.

Referencia WCAG 2.1:
  - 3:1   componentes de interfaz y bordes (1.4.11)
  - 4.5:1 texto normal (1.4.3)

    python scripts/verificar_contraste.py
"""

import re
import sys
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent / "src"
CSS = RAIZ / "styles.css"
CSS_EDITOR = RAIZ / "editor.css"

# (descripcion, token o color A, token o color B, ratio minimo)
EXIGENCIAS = [
    ("borde de celda vacia sobre el panel", "--borde-vacia", "--fondo", 3.0),
    ("borde de tecla sobre el panel", "--borde-tecla", "--fondo", 3.0),
    ("superficie de tecla sobre el panel", "--tecla", "--fondo", 1.8),
    ("tecla llena frente a celda vacia", "--tecla", "--vacia", 1.5),
    ("celda vacia sobre el panel", "--vacia", "--fondo", 1.08),
    ("hover frente a tecla normal", "--tecla-hover", "--tecla", 1.2),
    ("etiqueta sobre la tecla", "--texto", "--tecla", 4.5),
    ("texto tenue sobre la barra", "--texto-tenue", "--barra", 4.5),
    ("texto tenue sobre el panel", "--texto-tenue", "--fondo", 4.5),
    ("acento sobre el panel", "--acento", "--fondo", 3.0),
    # Tecla armada: la que espera confirmacion antes de apagar o borrar. Tiene
    # que distinguirse de una tecla normal sin depender solo del tono, porque
    # quien no distingue el rojo necesita verlo igual.
    ("borde de tecla armada sobre el panel", "--armada-borde", "--fondo", 3.0),
    ("tecla armada frente a una normal", "--armada", "--tecla", 1.2),
    ("etiqueta sobre la tecla armada", "--texto", "--armada", 4.5),
]

# La ventana del editor tiene colores propios, sin tokens. Se comprueban aparte
# porque un formulario con campos cuyo borde no se ve es igual de inservible que
# una rejilla sin bordes.
EXIGENCIAS_EDITOR = [
    ("borde de campo sobre su fondo", "#63666d", "#0d0f13", 3.0),
    ("borde de boton sobre el panel del editor", "#63656a", "#1c1f25", 1.5),
    ("borde de grupo sobre el fondo", "#44474e", "--fondo", 1.5),
    ("borde del bloque lateral sobre el fondo", "#4a4d54", "--fondo", 2.0),
    ("texto de campo sobre su fondo", "--texto", "#0d0f13", 4.5),
    ("texto de boton sobre el boton", "--texto", "#33363d", 4.5),
    ("marcador de posicion sobre su fondo", "#6b7076", "#0d0f13", 3.0),
]


def tokens(css: str) -> dict[str, str]:
    """Variables de color declaradas en :root."""
    bloque = re.search(r":root\s*\{(.*?)\}", css, re.S)
    if not bloque:
        sys.exit("no se encontro el bloque :root en styles.css")
    return {
        m.group(1): m.group(2)
        for m in re.finditer(r"(--[a-z-]+):\s*(#[0-9a-fA-F]{6})\s*;", bloque.group(1))
    }


def luminancia(color: str) -> float:
    h = color.lstrip("#")
    canales = [int(h[i : i + 2], 16) / 255 for i in (0, 2, 4)]
    lin = [c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4 for c in canales]
    return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2]


def ratio(a: str, b: str) -> float:
    la, lb = luminancia(a), luminancia(b)
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)


def main() -> int:
    declarados = tokens(CSS.read_text(encoding="utf-8"))
    fallos = 0

    hoja_editor = CSS_EDITOR.read_text(encoding="utf-8")
    for _, a, b, _ in EXIGENCIAS_EDITOR:
        for color in (a, b):
            if not color.startswith("--") and color not in hoja_editor:
                print(f"DESFASE  {color} ya no aparece en editor.css")
                fallos += 1

    print("--- panel ---")
    for fila in EXIGENCIAS + [None] + EXIGENCIAS_EDITOR:
        if fila is None:
            print("--- editor ---")
            continue
        descripcion, a, b, minimo = fila
        try:
            ca = declarados[a] if a.startswith("--") else a
            cb = declarados[b] if b.startswith("--") else b
        except KeyError as e:
            print(f"FALTA  el token {e} ya no existe en styles.css")
            fallos += 1
            continue

        r = ratio(ca, cb)
        if r >= minimo:
            print(f"OK    {r:5.2f}:1 (min {minimo})  {descripcion}")
        else:
            print(f"BAJO  {r:5.2f}:1 (min {minimo})  {descripcion}   {ca} vs {cb}")
            fallos += 1

    print()
    if fallos:
        print(f"{fallos} contraste(s) por debajo del minimo")
        return 1
    print("todos los contrastes cumplen")
    return 0


if __name__ == "__main__":
    sys.exit(main())
