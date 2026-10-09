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
 * @param {() => {surfaceId: string, page: number, pageCount: number}} contexto
 * @param {(msg: string, tipo?: string) => void} aviso
 * @param {() => Promise<void>} refrescar
 * @param {() => boolean} puedeArrastrar
 */
export function conectarReordenar(grid, contexto, aviso, refrescar, puedeArrastrar) {
  let origen = null;
  let arrastrando = false;
  let fantasma = null;

  /** Deshace el arrastre sin mover nada. */
  function cancelar() {
    if (!origen) return;
    origen.el.classList.remove("key--arrastrando");
    try {
      origen.el.releasePointerCapture?.(origen.puntero);
    } catch {
      // Si el puntero ya se solto, soltarlo otra vez no es un problema.
    }
    origen = null;
    arrastrando = false;
    fantasma?.remove();
    fantasma = null;
    limpiarResaltado();
  }

  /** El boton de pagina bajo unas coordenadas, si lo hay. */
  function pagerEn(x, y) {
    const el = document.elementFromPoint(x, y);
    const boton = /** @type {HTMLElement} */ (el)?.closest?.("#btn-prev, #btn-next");
    if (!boton) return null;
    return { el: boton, delta: boton.id === "btn-next" ? 1 : -1 };
  }

  grid.addEventListener("pointerdown", (ev) => {
    if (ev.button !== 0 || !puedeArrastrar()) return;
    const tecla = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-boton]");
    if (!tecla) return;
    origen = {
      botonId: tecla.getAttribute("data-boton"),
      x: ev.clientX,
      y: ev.clientY,
      el: tecla,
      puntero: ev.pointerId,
    };
    // Con captura, los movimientos siguen llegando aunque el cursor salga de la
    // ventana. Sin ella, soltar fuera dejaba el fantasma colgado para siempre.
    try {
      tecla.setPointerCapture?.(ev.pointerId);
    } catch {
      // Algun puntero no admite captura; el arrastre sigue valiendo sin ella.
    }
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
    const pagina = pagerEn(ev.clientX, ev.clientY);
    if (pagina) {
      pagina.el.classList.add("drop-aqui");
      return;
    }
    const destino = celdaEn(ev.clientX, ev.clientY);
    if (destino && destino.botonId !== origen.botonId) {
      destino.el.classList.add(destino.esCarpeta ? "drop-dentro" : "drop-aqui");
    }
  });

  // Windows cancela el puntero por su cuenta en mas casos de los que parece: un
  // gesto tactil que pasa a desplazamiento, otra ventana que roba la captura.
  // Sin esto, el fantasma se quedaba pegado al cursor hasta repintar.
  window.addEventListener("pointercancel", cancelar);

  // Escape es lo que cualquiera prueba al ver que ha cogido la tecla que no era.
  window.addEventListener("keydown", (ev) => {
    if (ev.key !== "Escape" || !origen) return;
    const movio = arrastrando;
    cancelar();
    if (movio) {
      // Se marca igualmente para que el `click` de soltar no ejecute la tecla.
      finDeArrastre = performance.now();
      aviso("Arrastre cancelado.", "info");
    }
  });

  window.addEventListener("pointerup", async (ev) => {
    if (!origen) return;
    const fuente = origen;
    const movio = arrastrando;
    cancelar();
    if (!movio) return;
    finDeArrastre = performance.now();

    const { surfaceId, page, pageCount } = contexto();

    // Soltar sobre el paginador manda la tecla a otra pagina: sin esto, mover una
    // tecla de pagina era imposible salvo editando deck.json.
    const pagina = pagerEn(ev.clientX, ev.clientY);
    if (pagina) {
      const destinoPagina = page + pagina.delta;
      if (destinoPagina < 0 || destinoPagina >= pageCount) {
        aviso("No hay página en esa dirección. Añade una desde el menú.");
        return;
      }
      try {
        await invoke("move_button", {
          buttonId: fuente.botonId,
          toSurface: surfaceId,
          toPage: destinoPagina,
          toPosition: 0,
        });
        await refrescar();
        aviso(`Tecla movida a la página ${destinoPagina + 1}.`, "info");
      } catch (e) {
        aviso(String(e));
      }
      return;
    }

    const destino = celdaEn(ev.clientX, ev.clientY);
    if (!destino || destino.botonId === fuente.botonId) return;

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
