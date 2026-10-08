// @ts-check
/**
 * Arranque del widget: carga el deck, pinta y conecta los eventos.
 *
 * Toda la logica de verdad (validar, lanzar, persistir) vive en Rust. Aqui solo
 * se pinta y se enrutan gestos.
 */

import {
  invoke,
  ventana,
  apiDisponible,
  modulosDisponibles,
  urlDeArchivo,
  ventanaApi,
} from "./api.js";
import { Nav } from "./nav.js";
import { render, renderMigas, setCatalogoSistema } from "./grid.js";
import { abrirMenu, cerrar as cerrarMenu, estaAbierto as menuAbierto } from "./menu.js";
import { conectarSoltarArchivos, conectarReordenar, huboArrastre } from "./dnd.js";

const $grid = /** @type {HTMLElement} */ (document.getElementById("grid"));
const $migas = /** @type {HTMLElement} */ (document.getElementById("breadcrumbs"));
const $pager = /** @type {HTMLElement} */ (document.getElementById("pager"));
const $pageLabel = /** @type {HTMLElement} */ (document.getElementById("page-label"));
const $prev = /** @type {HTMLButtonElement} */ (document.getElementById("btn-prev"));
const $next = /** @type {HTMLButtonElement} */ (document.getElementById("btn-next"));
const $pin = /** @type {HTMLButtonElement} */ (document.getElementById("btn-pin"));
const $min = /** @type {HTMLButtonElement} */ (document.getElementById("btn-min"));
const $close = /** @type {HTMLButtonElement} */ (document.getElementById("btn-close"));
const $toast = /** @type {HTMLElement} */ (document.getElementById("toast"));
const $repo = /** @type {HTMLButtonElement} */ (document.getElementById("btn-repo"));
const $lock = /** @type {HTMLButtonElement} */ (document.getElementById("btn-lock"));
const $titlebar = /** @type {HTMLElement} */ (document.getElementById("titlebar"));
const $panel = /** @type {HTMLElement} */ (document.getElementById("panel"));
const $pie = /** @type {HTMLElement} */ (document.getElementById("pie"));

/** @type {Nav | null} */
let nav = null;
/** Ids de botones con referencia rota, para marcarlos en la rejilla. */
let rotas = new Set();
/** Nivel de ventana vigente: "normal" | "top" | "desktop". */
let nivelActual = "top";
/** id de boton -> URL de su imagen, ya lista para un <img>. */
let urls = {};
let temporizadorToast = 0;

// --------------------------------------------------------------------- avisos

/**
 * @param {string} mensaje
 * @param {"error" | "info"} [tipo]
 */
function aviso(mensaje, tipo = "error") {
  $toast.textContent = mensaje;
  $toast.classList.toggle("toast--info", tipo === "info");
  $toast.hidden = false;
  clearTimeout(temporizadorToast);
  temporizadorToast = setTimeout(() => {
    $toast.hidden = true;
  }, tipo === "error" ? 6000 : 4000);
}

// --------------------------------------------------------------------- pintar

/** @param {"adelante" | "atras" | null} direccion */
function pintar(direccion = null) {
  if (!nav) return;
  // Un menu abierto sobre una rejilla que acaba de cambiar apuntaria a teclas que
  // ya no estan donde estaban.
  cerrarMenu();

  render($grid, nav, { rotas, urls });
  renderMigas($migas, nav);

  const paginas = nav.pageCount;
  $pager.hidden = paginas <= 1;
  $pageLabel.textContent = `Página ${nav.page + 1} / ${paginas}`;
  $prev.disabled = nav.page === 0;
  $next.disabled = nav.page >= paginas - 1;

  if (direccion) {
    const clase = direccion === "adelante" ? "anim-adelante" : "anim-atras";
    $grid.classList.remove("anim-adelante", "anim-atras");
    // Forzar reflujo para poder reiniciar la animacion en navegaciones seguidas.
    void $grid.offsetWidth;
    $grid.classList.add(clase);
  }
}

function aplicarAjustes(settings) {
  const raiz = document.documentElement;
  raiz.style.setProperty("--cols", String(settings.grid.cols));
  raiz.style.setProperty("--rows", String(settings.grid.rows));
  raiz.style.setProperty("--key", `${settings.key_size}px`);

  nivelActual = settings.window_level ?? "top";
  $pin.setAttribute("aria-pressed", String(nivelActual === "top"));
  $pin.title =
    nivelActual === "top"
      ? "Siempre encima (clic para pasar a nivel escritorio)"
      : nivelActual === "desktop"
        ? "Al nivel del escritorio (clic para ponerlo encima)"
        : "Ventana normal (clic para ponerlo encima)";
  $pin.classList.toggle("win-btn--escritorio", nivelActual === "desktop");

  const bloqueada = Boolean(settings.lock_position);
  $lock.setAttribute("aria-pressed", String(bloqueada));
  $lock.title = bloqueada ? "Posición bloqueada" : "Bloquear la posición";
  // Quitar el atributo es lo que desactiva el arrastre: Tauri solo mueve la
  // ventana desde elementos que lo llevan.
  if (bloqueada) $titlebar.removeAttribute("data-tauri-drag-region");
  else $titlebar.setAttribute("data-tauri-drag-region", "");

  // Tras pintar: la rejilla manda sobre el tamano de la ventana, no al reves.
  requestAnimationFrame(() => ajustarTamanoVentana(settings));
}

/**
 * Ajusta la ventana al tamano que pide la rejilla configurada.
 *
 * Sin esto, cambiar la rejilla a mas columnas de las que caben en una ventana de
 * ancho fijo recortaba las teclas sobrantes: quedaban guardadas en deck.json pero
 * invisibles e inalcanzables, que es peor que no dejar configurarla.
 *
 * Las medidas de la barra de titulo, el paginador y el pie se leen del DOM en vez
 * de codificarlas aqui, para que un cambio de CSS no vuelva a descuadrarlo.
 */
async function ajustarTamanoVentana(settings) {
  const cols = Math.max(1, settings.grid?.cols ?? 5);
  const filas = Math.max(1, settings.grid?.rows ?? 3);
  const lado = Math.max(32, settings.key_size ?? 96);

  const estiloRejilla = getComputedStyle($grid);
  const hueco = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--hueco")) || 10;
  const padX = parseFloat(estiloRejilla.paddingLeft) + parseFloat(estiloRejilla.paddingRight);
  const padY = parseFloat(estiloRejilla.paddingTop) + parseFloat(estiloRejilla.paddingBottom);

  // El panel lleva margen a los cuatro lados para que se vea su sombra.
  const margen = parseFloat(getComputedStyle($panel).marginLeft) * 2;
  const borde =
    parseFloat(getComputedStyle($panel).borderLeftWidth) +
    parseFloat(getComputedStyle($panel).borderRightWidth);

  const ancho = cols * lado + (cols - 1) * hueco + padX + margen + borde;
  const alto =
    filas * lado +
    (filas - 1) * hueco +
    padY +
    margen +
    borde +
    $titlebar.offsetHeight +
    ($pager.hidden ? 0 : $pager.offsetHeight) +
    $pie.offsetHeight;

  try {
    const { LogicalSize, currentMonitor } = ventanaApi();
    // No crecer mas alla del monitor: una ventana sin bordes mas grande que la
    // pantalla no se puede ni mover ni cerrar.
    let maxAncho = Infinity;
    let maxAlto = Infinity;
    const m = await currentMonitor();
    if (m) {
      const escala = m.scaleFactor || 1;
      maxAncho = m.size.width / escala;
      maxAlto = m.size.height / escala - 48; // margen para la barra de tareas
    }
    await ventana().setSize(
      new LogicalSize(Math.min(ancho, maxAncho), Math.min(alto, maxAlto))
    );
  } catch (err) {
    console.warn("[MiDeck] no se pudo ajustar el tamaño de la ventana:", err);
  }
}

// ----------------------------------------------------------------- navegacion

function volver() {
  if (nav?.back()) pintar("atras");
}

/** @param {string} botonId */
async function pulsar(botonId) {
  try {
    const salida = await invoke("run_action", { buttonId: botonId });
    if (salida?.navigate_to) {
      if (nav?.enter(salida.navigate_to)) pintar("adelante");
      else aviso("No se pudo entrar a la carpeta: la navegación sería circular.");
    }
  } catch (err) {
    rotas.add(botonId);
    pintar();
    aviso(String(err));
  }
}

// -------------------------------------------------------------------- eventos

function conectarEventos() {
  $grid.addEventListener("click", (ev) => {
    // Un clic que viene de terminar un arrastre no debe lanzar la accion.
    if (menuAbierto() || huboArrastre()) return;
    const tecla = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-boton], [data-accion]");
    if (!tecla) return;
    if (tecla.getAttribute("data-accion") === "volver") volver();
    else {
      const id = tecla.getAttribute("data-boton");
      if (id) pulsar(id);
    }
  });

  $migas.addEventListener("click", (ev) => {
    const crumb = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-nivel]");
    if (!crumb) return;
    const nivel = Number(crumb.getAttribute("data-nivel"));
    if (nav?.jumpTo(nivel)) pintar("atras");
  });

  $prev.addEventListener("click", () => {
    if (nav?.movePage(-1)) pintar("atras");
  });
  $next.addEventListener("click", () => {
    if (nav?.movePage(1)) pintar("adelante");
  });

  // La rueda sobre la rejilla cambia de pagina.
  $grid.addEventListener(
    "wheel",
    (ev) => {
      const delta = ev.deltaY > 0 ? 1 : -1;
      if (nav?.movePage(delta)) pintar(delta > 0 ? "adelante" : "atras");
    },
    { passive: true }
  );

  // Volver es la accion mas frecuente: tres atajos redundantes a proposito.
  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Backspace") {
      ev.preventDefault();
      volver();
    } else if (ev.key === "Tab" && ev.ctrlKey) {
      ev.preventDefault();
      const delta = ev.shiftKey ? -1 : 1;
      if (nav?.movePage(delta)) pintar(delta > 0 ? "adelante" : "atras");
    } else if (ev.key === "Escape") {
      $toast.hidden = true;
    } else if (ev.key === "F5") {
      ev.preventDefault();
      recargar();
    } else if (ev.key.toLowerCase() === "v" && ev.ctrlKey) {
      ev.preventDefault();
      if (teclaBajoCursor) {
        conMutacion(
          invoke("paste_image_to_button", { buttonId: teclaBajoCursor }),
          "Imagen pegada en la tecla."
        );
      } else {
        aviso("Pon el cursor sobre una tecla antes de pegar.", "info");
      }
    } else if (ev.key.toLowerCase() === "e" && ev.ctrlKey) {
      // Mientras no exista el editor visual (fase 3), este es el camino para
      // anadir teclas: abrir deck.json en el editor predeterminado.
      ev.preventDefault();
      invoke("open_config_file").catch((err) => aviso(String(err)));
    }
  });

  // Boton "atras" del raton (3) y clic central (1).
  document.addEventListener("auxclick", (ev) => {
    if (ev.button === 1 || ev.button === 3) {
      ev.preventDefault();
      volver();
    }
  });
  // El clic derecho sobre la rejilla abre el menu de edicion; fuera de ella,
  // simplemente no hay menu del sistema.
  document.addEventListener("contextmenu", (ev) => {
    if (!(/** @type {HTMLElement} */ (ev.target)?.closest?.("#grid, #breadcrumbs"))) {
      ev.preventDefault();
    }
  });

  // El pin alterna entre los dos niveles utiles. "Ventana normal" se elige en
  // Ajustes: es el caso raro y no merece un tercer estado en un boton de 30 px.
  $pin.addEventListener("click", async () => {
    const siguiente = nivelActual === "top" ? "desktop" : "top";
    try {
      await invoke("set_window_level", { level: siguiente });
      await refrescar();
      aviso(
        siguiente === "desktop"
          ? "Al nivel del escritorio: ya no tapa tus ventanas. Tráelo con el atajo global."
          : "Siempre encima de todo.",
        "info"
      );
    } catch (err) {
      aviso(String(err));
    }
  });

  $lock.addEventListener("click", async () => {
    const bloqueada = $lock.getAttribute("aria-pressed") === "true";
    try {
      await invoke("set_lock_position", { value: !bloqueada });
      await refrescar();
    } catch (err) {
      aviso(String(err));
    }
  });

  $repo.addEventListener("click", () =>
    invoke("open_repo").catch((err) => aviso(String(err)))
  );

  $min.addEventListener("click", () => ventana().minimize());
  // Cerrar dispara CloseRequested en Rust, que persiste la posicion.
  $close.addEventListener("click", () => ventana().close());
}

/**
 * Persistir la posicion al mover la ventana, con freno.
 *
 * Rust ya la guarda al cerrar, pero eso solo cubre el cierre ordenado: si el
 * proceso muere de otra forma, la posicion se perderia. Sin el freno se
 * escribiria deck.json en cada pixel de arrastre.
 */
async function vigilarPosicion() {
  let temporizador = 0;
  await ventana().onMoved(({ payload }) => {
    clearTimeout(temporizador);
    temporizador = setTimeout(() => {
      invoke("save_window_pos", { x: payload.x, y: payload.y }).catch((err) =>
        console.warn("[MiDeck] no se pudo guardar la posicion:", err)
      );
    }, 600);
  });
}

// ---------------------------------------------------------------- edicion

/** Contexto actual para las operaciones de edicion. */
function contextoActual() {
  return { surfaceId: nav?.surfaceId ?? "", page: nav?.page ?? 0 };
}

/** Recarga el deck desde Rust y repinta, conservando donde estaba el usuario. */
async function refrescar() {
  const vista = await invoke("get_deck");
  aplicarVista(vista);
  nav?.setDeck(vista.deck);
  pintar();
}

/** Aplica una vista que ya devolvio un comando de mutacion. */
function aplicarResultado(vista) {
  aplicarVista(vista);
  nav?.setDeck(vista.deck);
  pintar();
}

/**
 * @param {string} modo  "nuevo" | "editar" | "ajustes"
 * @param {string | null} botonId
 * @param {number} posicion
 */
function abrirEditor(modo, botonId, posicion) {
  const { surfaceId, page } = contextoActual();
  invoke("open_editor", {
    mode: modo,
    buttonId: botonId,
    surfaceId,
    page,
    position: posicion,
  }).catch((e) => aviso(String(e)));
}

async function conMutacion(promesa, exito) {
  try {
    const vista = await promesa;
    aplicarResultado(vista);
    if (exito) aviso(exito, "info");
  } catch (e) {
    aviso(String(e));
  }
}

/** Menu del clic derecho sobre una tecla o una celda vacia. */
function menuContextual(ev) {
  ev.preventDefault();
  const tecla = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-boton]");
  const vacia = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-celda]");
  const volver = /** @type {HTMLElement} */ (ev.target)?.closest?.('[data-accion="volver"]');
  if (volver) return;

  if (tecla) {
    const id = tecla.getAttribute("data-boton");
    const indice = Number(tecla.getAttribute("data-indice"));
    abrirMenu(ev.clientX, ev.clientY, [
      { label: "Editar…", accion: () => abrirEditor("editar", id, indice) },
      {
        label: "Pegar imagen del portapapeles",
        accion: () =>
          conMutacion(
            invoke("paste_image_to_button", { buttonId: id }),
            "Imagen pegada en la tecla."
          ),
      },
      {
        label: "Quitar la imagen",
        accion: () => quitarImagen(id),
      },
      { separador: true },
      {
        label: "Duplicar",
        accion: () => conMutacion(invoke("duplicate_button", { buttonId: id })),
      },
      {
        label: "Eliminar",
        peligro: true,
        accion: () => conMutacion(invoke("delete_button", { buttonId: id })),
      },
    ]);
    return;
  }

  if (vacia) {
    const indice = Number(vacia.getAttribute("data-celda"));
    abrirMenu(ev.clientX, ev.clientY, [
      { label: "Nueva tecla…", accion: () => abrirEditor("nuevo", null, indice) },
      {
        label: "Nueva carpeta",
        accion: () => nuevaCarpeta(indice),
      },
      { separador: true },
      { label: "Añadir una página", accion: () => añadirPagina() },
      {
        label: "Quitar esta página",
        desactivado: (nav?.pageCount ?? 1) <= 1,
        accion: () => quitarPagina(),
      },
      { separador: true },
      { label: "Ajustes…", accion: () => abrirEditor("ajustes", null, 0) },
    ]);
  }
}

/**
 * Quitar la imagen es volver la tecla a su icono automatico. La imagen sigue en
 * la biblioteca: limpiarla es una accion aparte, desde Ajustes.
 */
async function quitarImagen(botonId) {
  const b = buscarBotonLocal(botonId);
  if (!b) return;
  const copia = JSON.parse(JSON.stringify(b));
  copia.icon = { type: "auto", fit: copia.icon?.fit ?? "contain", label_style: copia.icon?.label_style ?? "below" };
  const { surfaceId, page } = contextoActual();
  await conMutacion(
    invoke("upsert_button", { surfaceId, page, button: copia })
  );
}

function buscarBotonLocal(id) {
  return nav?.buttons?.find((b) => b.id === id) ?? null;
}

function nuevaCarpeta(posicion) {
  const nombre = "Carpeta";
  const { surfaceId, page } = contextoActual();
  conMutacion(
    invoke("create_folder", { surfaceId, page, position: posicion, name: nombre }),
    "Carpeta creada. Clic derecho sobre ella para renombrarla desde el editor."
  );
}

function añadirPagina() {
  const { surfaceId } = contextoActual();
  conMutacion(invoke("add_page", { surfaceId }), "Página añadida.");
}

function quitarPagina() {
  const { surfaceId, page } = contextoActual();
  conMutacion(invoke("remove_page", { surfaceId, page }), "Página eliminada.");
}

/** Ctrl+V pega sobre la tecla que esta bajo el cursor. */
let teclaBajoCursor = null;

function conectarEdicion() {
  $grid.addEventListener("contextmenu", menuContextual);
  $migas.addEventListener("contextmenu", (ev) => {
    ev.preventDefault();
    abrirMenu(ev.clientX, ev.clientY, [
      { label: "Ajustes…", accion: () => abrirEditor("ajustes", null, 0) },
      { label: "Abrir deck.json", accion: () => invoke("open_config_file") },
      { label: "Recargar", accion: () => recargar() },
    ]);
  });

  $grid.addEventListener("pointermove", (ev) => {
    const t = /** @type {HTMLElement} */ (ev.target)?.closest?.("[data-boton]");
    teclaBajoCursor = t?.getAttribute("data-boton") ?? null;
  });
  $grid.addEventListener("pointerleave", () => {
    teclaBajoCursor = null;
  });

  // El panel se repinta cuando el editor guarda algo en su propia ventana.
  ventana()
    .listen("deck-changed", () => refrescar())
    .catch((e) => console.warn("[MiDeck] no se pudo escuchar deck-changed:", e));

  conectarSoltarArchivos(contextoActual, aviso, refrescar).catch((e) =>
    console.warn("[MiDeck] arrastre de archivos no disponible:", e)
  );
  conectarReordenar($grid, contextoActual, aviso, refrescar, () => !menuAbierto());
}

// -------------------------------------------------------------------- arranque

/**
 * Vuelca una vista recibida de Rust en el estado del frontend.
 * @param {any} vista
 */
function aplicarVista(vista) {
  aplicarAjustes(vista.deck.settings);
  rotas = new Set((vista.integrity?.broken ?? []).map((b) => b.button_id));
  // Rust entrega rutas absolutas; el webview solo puede cargarlas a traves del
  // protocolo asset:, y solo dentro del scope que Rust habilito al arrancar.
  urls = Object.fromEntries(
    Object.entries(vista.icon_paths ?? {}).map(([id, ruta]) => [
      id,
      urlDeArchivo(String(ruta)),
    ])
  );
  return problemasDe(vista);
}

/**
 * Resume en texto lo que haya que advertir de una carga.
 * @param {any} vista
 */
function problemasDe(vista) {
  const problemas = [...(vista.warnings ?? [])];
  const integridad = vista.integrity ?? {};
  if (integridad.broken?.length) {
    problemas.push(
      `${integridad.broken.length} tecla(s) de carpeta apuntan a una superficie que no existe.`
    );
  }
  if (integridad.cycles?.length) {
    problemas.push(
      `Hay ${integridad.cycles.length} ciclo(s) de carpetas en deck.json: ` +
        integridad.cycles.map((c) => c.join(" -> ")).join(" | ")
    );
  }
  if (integridad.orphans?.length) {
    problemas.push(
      `${integridad.orphans.length} carpeta(s) ya no son alcanzables desde la raiz; ` +
        "se conservan por si fue un borrado accidental."
    );
  }
  return problemas;
}

/** Relee deck.json del disco sin reiniciar el widget. */
async function recargar() {
  try {
    const vista = await invoke("reload_deck");
    const problemas = aplicarVista(vista);
    nav?.setDeck(vista.deck);
    pintar();
    aviso(
      problemas.length ? problemas.join(" ") : "deck.json recargado.",
      problemas.length ? "error" : "info"
    );
  } catch (err) {
    aviso(String(err));
  }
}

async function iniciar() {
  if (!apiDisponible()) {
    document.body.innerHTML =
      '<div style="padding:24px;font:13px Segoe UI,sans-serif;color:#e8eaed">' +
      "<b>No se encontro la API de Tauri.</b><br><br>" +
      "window.__TAURI__ expone: " +
      (modulosDisponibles().join(", ") || "(nada)") +
      "<br><br>Revisa app.withGlobalTauri en tauri.conf.json.</div>";
    return;
  }

  // El catalogo de acciones de Windows trae el icono de cada comando. Se pide
  // antes de la primera pintada para que ninguna tecla salga con el generico.
  setCatalogoSistema(await invoke("list_system_commands"));

  const vista = await invoke("get_deck");
  nav = new Nav(vista.deck);
  const problemas = aplicarVista(vista);

  conectarEventos();
  conectarEdicion();
  vigilarPosicion();
  pintar();

  if (problemas.length) aviso(problemas.join(" "), "error");

  console.log("[MiDeck] configuracion:", vista.config_path);
  console.log("[MiDeck] modulos de la API:", modulosDisponibles().join(", "));
}

iniciar().catch((err) => {
  aviso(`No se pudo arrancar: ${err}`);
  console.error(err);
});
