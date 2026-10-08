"""Genera el icono fuente de MiDeck.

Dibuja un panel oscuro con una rejilla 3 x 5 de teclas y una tecla de acento,
que es la silueta que identifica a un Stream Deck de un vistazo incluso a 16 px.
El .ico multi-resolucion lo produce despues `cargo tauri icon`, para no versionar
binarios que se pueden regenerar.
"""

from pathlib import Path

from PIL import Image, ImageDraw

LIENZO = 1024
FONDO = (26, 28, 31, 255)  # #1A1C1F
TECLA = (38, 41, 46, 255)  # #26292E
ACENTO = (76, 141, 255, 255)  # #4C8DFF

COLS, FILAS = 5, 3


def main() -> None:
    img = Image.new("RGBA", (LIENZO, LIENZO), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Cuerpo del panel, con margen para que respire en tamanos pequenos.
    margen = int(LIENZO * 0.06)
    d.rounded_rectangle(
        [margen, margen, LIENZO - margen, LIENZO - margen],
        radius=int(LIENZO * 0.17),
        fill=FONDO,
    )

    # Rejilla de teclas centrada dentro del panel.
    pad = int(LIENZO * 0.14)
    hueco = int(LIENZO * 0.028)
    ancho_util = LIENZO - 2 * pad
    alto_util = int(ancho_util * FILAS / COLS)
    top = (LIENZO - alto_util) // 2

    lado_x = (ancho_util - hueco * (COLS - 1)) / COLS
    lado_y = (alto_util - hueco * (FILAS - 1)) / FILAS

    for fila in range(FILAS):
        for col in range(COLS):
            x0 = pad + col * (lado_x + hueco)
            y0 = top + fila * (lado_y + hueco)
            # Una sola tecla encendida: da punto focal y lee bien a 16 px.
            encendida = (fila, col) == (1, 2)
            d.rounded_rectangle(
                [x0, y0, x0 + lado_x, y0 + lado_y],
                radius=int(lado_x * 0.22),
                fill=ACENTO if encendida else TECLA,
            )

    destino = Path(__file__).resolve().parent.parent / "assets" / "icon.png"
    destino.parent.mkdir(parents=True, exist_ok=True)
    img.save(destino)
    print(f"Icono fuente escrito en {destino}")


if __name__ == "__main__":
    main()
