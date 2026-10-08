// @ts-check
/**
 * Menu contextual del panel.
 *
 * Se usa el menu del clic derecho en vez de un modo de edicion aparte porque el
 * panel solo tiene 15 celdas: cualquier barra de herramientas robaria sitio a las
 * teclas, que es justo lo que no sobra.
 */

const $menu = /** @type {HTMLElement} */ (document.getElementById("ctxmenu"));

let cerrarActual = null;

function escapar(s) {
  return String(s).replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]
  );
}

/**
 * Abre el menu en unas coordenadas de pantalla.
 *
 * @param {number} x
 * @param {number} y
 * @param {Array<{label?: string, accion?: () => void, separador?: boolean, peligro?: boolean, desactivado?: boolean}>} items
 */
export function abrirMenu(x, y, items) {
  cerrar();

  $menu.innerHTML = items
    .map((it, i) => {
      if (it.separador) return '<div class="ctx-sep"></div>';
      const clases = ["ctx-item"];
      if (it.peligro) clases.push("ctx-item--peligro");
      const dis = it.desactivado ? " disabled" : "";
      return `<button class="${clases.join(" ")}" data-i="${i}"${dis}>${escapar(
        it.label ?? ""
      )}</button>`;
    })
    .join("");

  $menu.hidden = false;

  // Colocar dentro de la ventana: cerca de un borde, el menu se voltea en vez de
  // quedar cortado.
  const r = $menu.getBoundingClientRect();
  const maxX = window.innerWidth - r.width - 6;
  const maxY = window.innerHeight - r.height - 6;
  $menu.style.left = `${Math.max(6, Math.min(x, maxX))}px`;
  $menu.style.top = `${Math.max(6, Math.min(y, maxY))}px`;

  const alPulsar = (ev) => {
    const b = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-i]");
    if (!b) return;
    const it = items[Number(b.getAttribute("data-i"))];
    cerrar();
    it?.accion?.();
  };
  const alSalir = (ev) => {
    if (!$menu.contains(/** @type {Node} */ (ev.target))) cerrar();
  };
  const alTeclear = (ev) => {
    if (ev.key === "Escape") cerrar();
  };

  $menu.addEventListener("click", alPulsar);
  // En captura, para adelantarse a los manejadores de la rejilla.
  document.addEventListener("pointerdown", alSalir, true);
  document.addEventListener("keydown", alTeclear);

  cerrarActual = () => {
    $menu.hidden = true;
    $menu.innerHTML = "";
    $menu.removeEventListener("click", alPulsar);
    document.removeEventListener("pointerdown", alSalir, true);
    document.removeEventListener("keydown", alTeclear);
    cerrarActual = null;
  };
}

export function cerrar() {
  cerrarActual?.();
}

export function estaAbierto() {
  return !$menu.hidden;
}
