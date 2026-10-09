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
const $menu = /** @type {HTMLButtonElement} */ (document.getElementById("btn-menu"));
const $perfil = /** @type {HTMLButtonElement} */ (document.getElementById("btn-perfil"));
const $titlebar = /** @type {HTMLElement} */ (document.getElementById("titlebar"));
const $panel = /** @type {HTMLElement} */ (document.getElementById("panel"));
const $pie = /** @type {HTMLElement} */ (document.getElementById("pie"));

/** @type {Nav | null} */
let nav = null;
/** Ids de botones con referencia rota, para marcarlos en la rejilla. */
let rotas = new Set();

/**
 * Teclas que piden pulsar dos veces, y cual esta armada ahora mismo.
 *
 * Quien decide que es peligroso es Rust, no esta lista: aqui solo se consulta la
 * pertenencia, igual que con `rotas`. Asi una accion destructiva nueva queda
 * protegida por declararla en el catalogo, sin tocar este archivo.
 */
let aConfirmar = new Set();
let armada = null;
let temporizadorArmada = null;

/** Desarma la tecla que estuviera esperando confirmacion. */
function desarmar(repintar = true) {
  if (armada === null) return;
  armada = null;
  clearTimeout(temporizadorArmada);
  if (repintar) pintar();
}
/** Nivel de ventana vigente: "normal" | "top" | "desktop". */
let nivelActual = "top";
/**
 * Perfil activo segun Rust: `{ exe, surface, nombre }`. `surface` en null
 * significa que ninguna aplicacion configurada esta delante.
 */
let perfilActivo = { exe: null, surface: null, nombre: null };

/**
 * El perfil que el panel esta ensenando, que no siempre es el detectado.
 *
 * Con el ancla puesta son distintos: se sigue viendo el de antes mientras el
 * sistema detecta otro. El ancla tiene que hablar del que se ve, no del que se
 * detecta, o diria «perfil Word fijado» con las teclas de Excel delante. Y es lo
 * que permite ensenar un perfil cuya aplicacion no esta abierta, para editarlo.
 */
let perfilMostrado = { surface: null, nombre: null };

/**
 * Con el ancla puesta, el panel deja de seguir a la aplicacion en primer plano.
 *
 * Es estado de sesion y no se guarda: un ancla que sobrevive al reinicio deja a
 * alguien atrapado en un perfil sin saber por que.
 */
let perfilFijado = false;
/** id de tecla -> lo que ensena del sistema. Lo manda Rust, no se calcula aqui. */
let vivo = {};
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

  render($grid, nav, { rotas, urls, armada, vivo });
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

/**
 * Aplica el perfil que Rust acaba de resolver.
 *
 * @param {{exe?: string, surface?: string|null, nombre?: string|null}} payload
 */
function aplicarPerfil(payload) {
  perfilActivo = {
    exe: payload?.exe ?? null,
    surface: payload?.surface ?? null,
    nombre: payload?.nombre ?? null,
  };
  // Con el ancla puesta el panel deja de seguir a la aplicacion, pero el perfil
  // activo se sigue sabiendo: al soltarla se va al que toque sin esperar a que
  // cambies de ventana.
  if (perfilFijado || !nav) {
    pintarAncla();
    return;
  }

  mostrarPerfil(perfilActivo.surface, perfilActivo.nombre);
}

/** Lleva el panel a un perfil y deja constancia de cual se esta viendo. */
function mostrarPerfil(surface, nombre) {
  perfilMostrado = { surface: surface ?? null, nombre: nombre ?? null };
  pintarAncla();
  if (!nav) return;
  if (nav.setBase(surface ?? nav.deck.root)) pintar();
}

/** El ancla solo existe cuando hay perfil, y su estado se lee de un vistazo. */
function pintarAncla() {
  const hay = Boolean(perfilMostrado.surface);
  $perfil.hidden = !hay;
  if (!hay) {
    // Sin perfil no hay nada que fijar: soltarla evita quedarse anclado a nada.
    perfilFijado = false;
    return;
  }
  $perfil.setAttribute("aria-pressed", String(perfilFijado));
  $perfil.title = perfilFijado
    ? `Perfil «${perfilMostrado.nombre}» fijado (clic para soltarlo)`
    : `Perfil «${perfilMostrado.nombre}» (clic para fijarlo y que no cambie solo)`;
}

/** Conmuta el ancla y vuelve al perfil que toque si se acaba de soltar. */
function alternarAncla() {
  perfilFijado = !perfilFijado;
  pintarAncla();
  if (!perfilFijado) {
    mostrarPerfil(perfilActivo.surface, perfilActivo.nombre);
  }
  aviso(
    perfilFijado
      ? "Perfil fijado: el panel ya no cambiará al pasar de aplicación."
      : "Perfil suelto: el panel vuelve a seguir a la aplicación.",
    "info"
  );
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

/**
 * Muescas de rueda pendientes, agrupadas antes de molestar a Rust.
 *
 * Una vuelta de rueda son decenas de eventos en medio segundo. Llamar una vez por
 * muesca lanzaria decenas de entradas sinteticas y las respuestas llegarian
 * desordenadas; juntarlas en una llamada con un contador sale igual de fino en
 * pantalla y no castiga la maquina.
 */
let ruedaPendiente = null;
let temporizadorRueda = 0;

/**
 * @param {string} botonId
 * @param {boolean} arriba
 * @param {number} delta Magnitud del evento, que no siempre es una sola muesca.
 */
function acumularRueda(botonId, arriba, delta) {
  // Cambiar de tecla o de sentido vacia lo acumulado: mezclar dos sentidos en una
  // llamada daria un resultado que no se corresponde con el gesto.
  if (
    ruedaPendiente &&
    (ruedaPendiente.botonId !== botonId || ruedaPendiente.arriba !== arriba)
  ) {
    enviarRueda();
  }
  // Se suma el delta y se recuerda el mas pequeno visto, que es lo que vale una
  // muesca en este dispositivo. Contar eventos seria mas simple pero no vale para
  // todos: un panel tactil de precision manda muchos eventos de delta pequeno en
  // vez de muescas enteras, y contarlos como muescas dispararia la accion decenas
  // de veces por un gesto corto. Normalizar por la unidad mas pequena del gesto
  // sale igual de exacto con una rueda y correcto con un panel tactil.
  ruedaPendiente = ruedaPendiente ?? { botonId, arriba, total: 0, unidad: Infinity };
  ruedaPendiente.total += delta || 1;
  ruedaPendiente.unidad = Math.min(ruedaPendiente.unidad, delta || 1);

  if (temporizadorRueda) return;
  temporizadorRueda = setTimeout(enviarRueda, 60);
}

function enviarRueda() {
  clearTimeout(temporizadorRueda);
  temporizadorRueda = 0;
  const pendiente = ruedaPendiente;
  ruedaPendiente = null;
  if (!pendiente) return;

  const veces = Math.max(1, Math.round(pendiente.total / pendiente.unidad));
  invoke("run_wheel", {
    buttonId: pendiente.botonId,
    arriba: pendiente.arriba,
    veces,
  }).catch((e) => aviso(String(e)));
}

/** Etiqueta de una tecla segun el deck, que es la de verdad. */
function etiquetaDe(id) {
  return nav?.buttons?.find((b) => b.id === id)?.label ?? "";
}

/**
 * Tecla copiada o cortada, esperando a que la peguen.
 *
 * Existe porque arrastrar no cruza fronteras: dentro de una carpeta no se puede
 * llevar una tecla al nivel de arriba, porque ese nivel no esta en pantalla. Vive
 * en memoria y sobrevive a entrar y salir de carpetas, que es justo el viaje para
 * el que hace falta.
 *
 * @type {{id: string, modo: "copiar" | "cortar", etiqueta: string} | null}
 */
let portapapelesTecla = null;

/**
 * @param {string} id
 * @param {"copiar" | "cortar"} modo
 * @param {string} etiqueta
 */
function tomarTecla(id, modo, etiqueta) {
  portapapelesTecla = { id, modo, etiqueta };
  aviso(
    `«${etiqueta || "Tecla"}» ${modo === "cortar" ? "cortada" : "copiada"}. ` +
      "Clic derecho en una celda libre para pegarla.",
    "info"
  );
}

/** Pega lo que haya en el portapapeles en una celda de la pagina actual. */
async function pegarTecla(indice) {
  if (!portapapelesTecla) return;
  const { id, modo, etiqueta } = portapapelesTecla;
  const { surfaceId, page } = contextoActual();

  try {
    if (modo === "cortar") {
      await invoke("move_button", {
        buttonId: id,
        toSurface: surfaceId,
        toPage: page,
        toPosition: indice,
      });
      // Cortar se gasta al pegar: dejarlo cargado invitaria a pegar dos veces la
      // misma tecla, y la segunda fallaria porque ya no esta donde estaba.
      portapapelesTecla = null;
    } else {
      await invoke("copy_button", {
        buttonId: id,
        toSurface: surfaceId,
        toPage: page,
        toPosition: indice,
      });
    }
    await refrescar();
    aviso(`«${etiqueta || "Tecla"}» pegada.`, "info");
  } catch (e) {
    aviso(String(e));
  }
}

/** @param {string} botonId */
async function pulsar(botonId) {
  // Apagar o vaciar la papelera por un clic de mas no tiene vuelta atras. La
  // primera pulsacion arma la tecla y la segunda ejecuta; pulsar otra cualquiera
  // la desarma, igual que esperar tres segundos.
  if (aConfirmar.has(botonId)) {
    if (armada !== botonId) {
      clearTimeout(temporizadorArmada);
      armada = botonId;
      pintar();
      temporizadorArmada = setTimeout(() => desarmar(), 3000);
      return;
    }
  }
  // Siempre con repintado: desarmar por dentro y dejar la tecla roja en pantalla
  // hace creer que sigue esperando confirmacion, que es justo lo contrario de lo
  // que se quiere de un aviso.
  desarmar();

  try {
    const salida = await invoke("run_action", { buttonId: botonId });
    // Una captura que no dice donde quedo el archivo es una captura perdida.
    if (salida?.message) aviso(salida.message, "info");
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

  // La rueda: sobre una tecla con rueda, la gira; sobre cualquier otra cosa de la
  // rejilla, cambia de pagina, que es lo que hacia siempre.
  //
  // El reparto va dentro del mismo manejador a proposito. Con dos manejadores, el
  // de la tecla no podria evitar que el de la pagina se disparara tambien, y girar
  // el volumen cambiaria de pagina a la vez.
  $grid.addEventListener(
    "wheel",
    (ev) => {
      const arriba = ev.deltaY < 0;
      const id = /** @type {HTMLElement} */ (ev.target)
        ?.closest?.("[data-boton]")
        ?.getAttribute("data-boton");
      const tecla = id ? nav?.buttons?.find((b) => b.id === id) : null;

      if (tecla?.wheel) {
        acumularRueda(id, arriba, Math.abs(ev.deltaY));
        return;
      }
      const delta = arriba ? -1 : 1;
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

  // Ajustes y edicion estaban solo en el clic derecho sobre una celda vacia, asi
  // que con la rejilla llena no habia forma de llegar: un usuario lo reporto. El
  // boton de la barra de titulo da una via que no depende de que sobre un hueco.
  $perfil.addEventListener("click", alternarAncla);

  $menu.addEventListener("click", (ev) => {
    ev.stopPropagation();
    if (menuAbierto()) {
      cerrarMenu();
      return;
    }
    const r = $menu.getBoundingClientRect();
    const libre = primeraCeldaLibre();
    const sinHueco = libre === null;
    abrirMenu(r.left, r.bottom + 4, [
      {
        label: "Nueva tecla…",
        desactivado: sinHueco,
        accion: () => abrirEditor("nuevo", null, libre ?? 0),
      },
      {
        label: "Nueva carpeta",
        desactivado: sinHueco,
        accion: () => nuevaCarpeta(libre ?? 0),
      },
      { separador: true },
      { label: "Añadir una página", accion: () => añadirPagina() },
      {
        label: "Quitar esta página",
        desactivado: (nav?.pageCount ?? 1) <= 1,
        accion: () => quitarPagina(),
      },
      { separador: true },
      {
        label: perfilFijado ? "Soltar el perfil" : "Fijar este perfil",
        desactivado: !perfilActivo.surface,
        accion: () => alternarAncla(),
      },
      { separador: true },
      { label: "Ajustes…", accion: () => abrirEditor("ajustes", null, 0) },
      { label: "Abrir deck.json", accion: () => invoke("open_config_file") },
      { label: "Recargar", accion: () => recargar() },
    ]);
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
  return {
    surfaceId: nav?.surfaceId ?? "",
    page: nav?.page ?? 0,
    // Lo necesita el arrastre para no mandar una tecla a una pagina que no existe.
    pageCount: nav?.pageCount ?? 1,
  };
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
        label: "Copiar",
        accion: () => tomarTecla(id, "copiar", etiquetaDe(id)),
      },
      {
        label: "Cortar",
        accion: () => tomarTecla(id, "cortar", etiquetaDe(id)),
      },
      {
        label: "Duplicar",
        accion: () => conMutacion(invoke("duplicate_button", { buttonId: id })),
      },
      {
        label: "Eliminar",
        peligro: true,
        accion: () => conMutacion(invoke("delete_button", { buttonId: id })),
      },
      { separador: true },
      // Tambien aqui, y no solo sobre una celda vacia: con la rejilla llena no
      // quedaba ninguna celda sobre la que hacer clic derecho.
      { label: "Ajustes…", accion: () => abrirEditor("ajustes", null, 0) },
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
      {
        label: portapapelesTecla
          ? `Pegar «${portapapelesTecla.etiqueta || "tecla"}»`
          : "Pegar",
        desactivado: !portapapelesTecla,
        accion: () => pegarTecla(indice),
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

/**
 * Primera celda libre de la pagina que se esta viendo, o null si no queda.
 *
 * Se mira el DOM y no `nav.buttons` porque al pintar se recolocan los botones
 * cuya posicion choca: lo que de verdad esta libre es lo que la rejilla muestra
 * como hueco, no lo que diga el archivo.
 */
function primeraCeldaLibre() {
  const vacia = $grid.querySelector("[data-celda]");
  return vacia ? Number(vacia.getAttribute("data-celda")) : null;
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
    // Si el cursor se fue de la rejilla, la confirmacion ya no viene: dejar una
    // tecla armada esperando es pedir un accidente la proxima vez que se pase
    // por encima.
    desarmar();
  });

  // El panel se repinta cuando el editor guarda algo en su propia ventana.
  ventana()
    .listen("deck-changed", () => refrescar())
    .catch((e) => console.warn("[MiDeck] no se pudo escuchar deck-changed:", e));

  // Rust avisa cuando la aplicacion en primer plano cambia de verdad, ya
  // amortiguada, y con el perfil ya resuelto.
  ventana()
    .listen("perfil-activo", (ev) => aplicarPerfil(ev?.payload))
    .catch((e) => console.warn("[MiDeck] no se pudo escuchar perfil-activo:", e));

  // Ajustes pide ensenar un perfil para editarlo. Se fija el ancla: su aplicacion
  // no tiene por que estar abierta, y sin fijarla el panel se marcharia en cuanto
  // el usuario tocara cualquier otra ventana.
  //
  // Cada escucha va en su propia llamada: `listen` devuelve una promesa, no la
  // ventana, asi que encadenar dos seguidas lanza y se lleva por delante todo lo
  // que venga despues de `conectarEventos`, incluida la primera pintada.
  // El latido de Rust trae solo las teclas con fuente, y solo cuando algo cambio.
  ventana()
    .listen("estado-vivo", (ev) => {
      vivo = ev?.payload ?? {};
      pintar();
    })
    .catch((e) => console.warn("[MiDeck] no se pudo escuchar estado-vivo:", e));

  ventana()
    .listen("editar-perfil", (ev) => {
      perfilFijado = true;
      mostrarPerfil(ev?.payload?.surface, ev?.payload?.nombre);
      aviso(
        `Editando «${ev?.payload?.nombre ?? ""}». El panel está fijado: ` +
          "suelta el ancla para que vuelva a seguir a la aplicación.",
        "info"
      );
    })
    .catch((e) => console.warn("[MiDeck] no se pudo escuchar editar-perfil:", e));

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
  aConfirmar = new Set(vista.confirm_required ?? []);
  armada = null;
  clearTimeout(temporizadorArmada);
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
  console.error(err);
  // El aviso normal se desvanece a los pocos segundos, y si el arranque se rompe
  // la rejilla se queda vacia: lo que se ve es un panel negro sin explicacion, y
  // el motivo ya se ha ido. Aqui el error se queda escrito donde deberian estar
  // las teclas, que es donde se mira.
  $grid.innerHTML =
    '<div class="arranque-roto">' +
    "<b>MiDeck no pudo arrancar.</b><br><br>" +
    escaparTexto(String(err)) +
    "<br><br>Vuelve a abrirlo. Si sigue igual, manda este texto." +
    "</div>";
  aviso(`No se pudo arrancar: ${err}`);
});

/** Escapa para meter texto en innerHTML sin que el error se interprete. */
function escaparTexto(t) {
  return String(t).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]
  );
}
