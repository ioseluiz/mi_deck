// @ts-check
/**
 * Arrastrar y soltar: desde el Explorador, y reordenar dentro de la rejilla.
 *
 * TRAMPA IMPORTANTE: con `dragDropEnabled: true` en tauri.conf.json, Windows
 * deshabilita el drag-and-drop HTML5 dentro del webview. Por eso el reordenado
 * interno va con eventos de puntero (pointerdown/pointermove/pointerup) y no con
 * la API de arrastre del navegador, que aqui simplemente no dispara.
 */

import { invoke, ventana } from "./api.js";

/** Pixeles que hay que mover el raton antes de considerarlo un arrastre. */
const UMBRAL = 7;

/**
 * Celda de la rejilla bajo unas coordenadas de cliente.
 * @param {number} x
 * @param {number} y
 */
function celdaEn(x, y) {
  const el = document.elementFromPoint(x, y);
  const tecla = el?.closest?.("[data-boton]");
  const vacia = el?.closest?.("[data-celda]");
  if (tecla) {
    return {
      indice: Number(tecla.getAttribute("data-indice")),
      botonId: tecla.getAttribute("data-boton"),
      esCarpeta: tecla.classList.contains("key--carpeta"),
      el: /** @type {HTMLElement} */ (tecla),
    };
  }
  if (vacia) {
    return {
      indice: Number(vacia.getAttribute("data-celda")),
      botonId: null,
      esCarpeta: false,
      el: /** @type {HTMLElement} */ (vacia),
    };
  }
  return null;
}

function limpiarResaltado() {
  for (const el of document.querySelectorAll(".drop-aqui, .drop-imagen, .drop-dentro")) {
    el.classList.remove("drop-aqui", "drop-imagen", "drop-dentro");
  }
}

// ------------------------------------------------- soltar desde el Explorador

/**
 * @param {() => {surfaceId: string, page: number}} contexto
 * @param {(msg: string, tipo?: string) => void} aviso
 * @param {() => Promise<void>} refrescar
 */
export async function conectarSoltarArchivos(contexto, aviso, refrescar) {
  await ventana().onDragDropEvent(async (ev) => {
    const p = ev.payload;

    if (p.type === "leave") {
      limpiarResaltado();
      return;
    }

    // Tauri entrega la posicion en pixeles fisicos; el DOM trabaja en logicos.
    const dpr = window.devicePixelRatio || 1;
    const x = (p.position?.x ?? 0) / dpr;
    const y = (p.position?.y ?? 0) / dpr;
    const destino = celdaEn(x, y);

    if (p.type === "over" || p.type === "enter") {
      limpiarResaltado();
      if (!destino) return;
      // Preguntar a Rust que significaria soltar aqui, para resaltar en
      // consecuencia: el resultado no debe ser nunca una sorpresa.
      const rutas = p.paths ?? [];
      const efecto = await invoke("preview_drop", {
        paths: rutas,
        onButton: Boolean(destino.botonId),
        isFolderKey: destino.esCarpeta,
      });
      const clase =
        efecto === "imagen" ? "drop-imagen" : efecto === "dentro" ? "drop-dentro" : "drop-aqui";
      destino.el.classList.add(clase);
      return;
    }

    if (p.type !== "drop") return;
    limpiarResaltado();

    const rutas = p.paths ?? [];
    if (!rutas.length || !destino) return;

    const { surfaceId, page } = contexto();
    try {
      const efecto = await invoke("preview_drop", {
        paths: rutas,
        onButton: Boolean(destino.botonId),
        isFolderKey: destino.esCarpeta,
      });

      if (efecto === "imagen" && destino.botonId) {
        await invoke("set_button_image", {
          buttonId: destino.botonId,
          path: rutas[0],
        });
        aviso("Imagen aplicada a la tecla.", "info");
      } else if (efecto === "dentro" && destino.botonId) {
        // Soltar sobre una tecla de carpeta mete el contenido dentro, sin entrar.
        const sub = await invoke("surface_of_button", { buttonId: destino.botonId });
        await invoke("drop_paths", {
          surfaceId: sub,
          page: 0,
          position: 1,
          paths: rutas,
        });
        aviso(`${rutas.length} tecla(s) añadidas dentro de la carpeta.`, "info");
      } else {
        await invoke("drop_paths", {
          surfaceId,
          page,
          position: destino.indice,
          paths: rutas,
        });
        aviso(`${rutas.length} tecla(s) creadas.`, "info");
      }
      await refrescar();
    } catch (e) {
      aviso(String(e));
    }
  });
}

// ------------------------------------------------- reordenar dentro del panel

/**
 * Arrastre interno con eventos de puntero.
 *
 * @param {HTMLElement} grid
 * @param {() => {surfaceId: string, page: number}} contexto
 * @param {(msg: string, tipo?: string) => void} aviso
 * @param {() => Promise<void>} refrescar
 * @param {() => boolean} puedeArrastrar
 */
export function conectarReordenar(grid, contexto, aviso, refrescar, puedeArrastrar) {
  let origen = null;
  let arrastrando = false;
  let fantasma = null;

  grid.addEventListener("pointerdown", (ev) => {
    if (ev.button !== 0 || !puedeArrastrar()) return;
    const tecla = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-boton]");
    if (!tecla) return;
    origen = {
      botonId: tecla.getAttribute("data-boton"),
      x: ev.clientX,
      y: ev.clientY,
      el: tecla,
    };
  });

  window.addEventListener("pointermove", (ev) => {
    if (!origen) return;

    if (!arrastrando) {
      const d = Math.hypot(ev.clientX - origen.x, ev.clientY - origen.y);
      if (d < UMBRAL) return;
      arrastrando = true;
      origen.el.classList.add("key--arrastrando");

      fantasma = origen.el.cloneNode(true);
      fantasma.classList.add("key--fantasma");
      document.body.appendChild(fantasma);
    }

    fantasma.style.left = `${ev.clientX}px`;
    fantasma.style.top = `${ev.clientY}px`;

    limpiarResaltado();
    const destino = celdaEn(ev.clientX, ev.clientY);
    if (destino && destino.botonId !== origen.botonId) {
      destino.el.classList.add(destino.esCarpeta ? "drop-dentro" : "drop-aqui");
    }
  });

  window.addEventListener("pointerup", async (ev) => {
    if (!origen) return;
    const fuente = origen;
    const movio = arrastrando;
    origen = null;
    arrastrando = false;
    fuente.el.classList.remove("key--arrastrando");
    fantasma?.remove();
    fantasma = null;
    limpiarResaltado();
    if (!movio) return;
    finDeArrastre = performance.now();

    const destino = celdaEn(ev.clientX, ev.clientY);
    if (!destino || destino.botonId === fuente.botonId) return;

    const { surfaceId, page } = contexto();
    try {
      if (destino.esCarpeta && destino.botonId) {
        // Soltar una tecla sobre una carpeta la mete dentro.
        const sub = await invoke("surface_of_button", { buttonId: destino.botonId });
        await invoke("move_button", {
          buttonId: fuente.botonId,
          toSurface: sub,
          toPage: 0,
          toPosition: 1,
        });
      } else {
        await invoke("move_button", {
          buttonId: fuente.botonId,
          toSurface: surfaceId,
          toPage: page,
          toPosition: destino.indice,
        });
      }
      await refrescar();
    } catch (e) {
      aviso(String(e));
    }
  });
}

/**
 * Al soltar, el navegador emite ademas un `click`. Sin esta marca, reordenar una
 * tecla tambien lanzaria su accion, que es justo lo que no quiere quien arrastra.
 */
let finDeArrastre = 0;

export function huboArrastre() {
  return performance.now() - finDeArrastre < 350;
}
