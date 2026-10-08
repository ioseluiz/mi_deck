// @ts-check
/**
 * Ventana del editor: edita una tecla o muestra los ajustes.
 *
 * Vive en su propia ventana porque el panel mide 596 x 479 y no es
 * redimensionable. El contexto (modo, superficie, pagina, celda, boton) se pide a
 * Rust con `editor_context` nada mas cargar.
 *
 * Al guardar llama a los comandos de Rust, avisa al panel con el evento
 * `deck-changed` y se cierra. El deck nunca se edita desde aqui en memoria: la
 * unica fuente de verdad sigue siendo Rust.
 */

import { invoke, urlDeArchivo, apiDisponible } from "./api.js";
import { iconoPorAccion, setCatalogoSistema } from "./grid.js";

/**
 * Catalogo de acciones de Windows tal cual lo devuelve Rust: una lista de
 * grupos, cada uno con su etiqueta y sus comandos. El editor no escribe ni un
 * nombre de accion ni un titulo de grupo.
 */
let CATALOGO_SISTEMA = [];

/** Ficha de un comando del catalogo, o null si no esta. */
function fichaSistema(id) {
  for (const g of CATALOGO_SISTEMA) {
    const c = (g.comandos ?? []).find((x) => x.id === id);
    if (c) return c;
  }
  return null;
}

// El contexto llega por comando, no por la URL: Tauri resuelve el recurso
// incluyendo la cadena de consulta, asi que editor.html?modo=... no existe como
// archivo y la ventana saldria en blanco.
let MODO = "nuevo";
let SURFACE = "";
let PAGINA = 0;
let POSICION = 0;
let BOTON_ID = null;

const $ = (id) => /** @type {any} */ (document.getElementById(id));

/** Estado de la tecla que se esta editando. */
let tecla = {
  id: BOTON_ID ?? "",
  position: POSICION,
  label: "",
  icon: { type: "auto", fit: "contain", label_style: "below" },
  action: { type: "app", target: "", args: "", workdir: "", focus_if_running: false },
};

/** Ajustes, cuando el modo es "ajustes". */
let settings = null;
let autostartInicial = false;

/**
 * Icono que Windows da al destino actual, para la vista previa.
 *
 * El panel ya pinta el icono real de cada aplicacion; aqui se pedia aparte
 * porque el editor solo tenia el destino a medio escribir. Vive fuera de
 * `tecla.icon` a proposito: es algo que se ve, no algo que se guarde.
 */
let PREVIA_AUTO = null;

/** Tipos de accion de los que Windows sabe sacar un icono. */
const CON_ICONO_PROPIO = new Set(["app", "path", "script"]);

// ------------------------------------------------------------------- utiles

function error(msg) {
  const el = $("ed-error");
  el.textContent = msg ?? "";
  el.hidden = !msg;
}

function escapar(s) {
  return String(s).replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]
  );
}

// ---------------------------------------------------------- vista previa

/**
 * Pinta la tecla tal y como se vera en el panel, usando la misma clase .key.
 * Es la diferencia entre acertar al primer intento y guardar a ciegas para ir a
 * mirar como quedo.
 */
function pintarPrevia() {
  const prev = $("ed-preview");
  const icon = tecla.icon ?? {};
  const clases = ["key"];
  if (tecla.action?.type === "folder") clases.push("key--carpeta");
  if (icon.fit === "cover") clases.push("key--cover");
  if (icon.label_style === "overlay") clases.push("key--label-overlay");
  if (icon.label_style === "none") clases.push("key--label-none");
  prev.className = clases.join(" ");
  prev.style.background = icon.background || "";

  let contenido = "";
  if (icon.type === "emoji" && icon.char) {
    contenido = escapar(icon.char);
  } else if (icon.type === "image" && icon.src) {
    contenido = `<img src="${escapar(icon.src)}" alt="">`;
  } else if (PREVIA_AUTO && CON_ICONO_PROPIO.has(tecla.action?.type)) {
    // El mismo icono que Windows le da al destino, que es el que acabara
    // saliendo en el panel. Antes aqui habia un cuadrado generico y no se sabia
    // como iba a quedar la tecla hasta guardarla.
    contenido = `<img src="${escapar(PREVIA_AUTO)}" alt="">`;
  } else if (tecla.action?.type === "url") {
    const dom = (tecla.action.target || "").split("//").pop()?.split("/")[0] ?? "?";
    let h = 0;
    for (const c of dom) h = (h * 31 + c.charCodeAt(0)) % 360;
    contenido = `<span class="pastilla" style="background:hsl(${h} 48% 40%)">${escapar(
      (dom[0] || "?").toUpperCase()
    )}</span>`;
  } else {
    // La misma funcion que usa el panel: antes habia aqui un SVG escrito a mano
    // y la vista previa se quedaba atras cada vez que se anadia un icono.
    contenido = iconoPorAccion(tecla.action);
  }

  prev.innerHTML =
    `<span class="key-icono">${contenido}</span>` +
    `<span class="key-etiqueta">${escapar(tecla.label || "")}</span>`;
}

// ------------------------------------------------------- formulario de tecla

/**
 * Pide a Windows el icono del destino actual y repinta la vista previa.
 *
 * Va con retardo porque el campo de destino dispara en cada tecla y extraer un
 * icono toca el disco: sin esto, escribir una ruta larga lanzaria una extraccion
 * por caracter.
 */
let temporizadorIcono = null;
function refrescarIconoAuto() {
  clearTimeout(temporizadorIcono);
  temporizadorIcono = setTimeout(async () => {
    const tipo = tecla.action?.type;
    if (!CON_ICONO_PROPIO.has(tipo)) {
      PREVIA_AUTO = null;
      pintarPrevia();
      return;
    }
    const destino = tecla.action?.target ?? "";
    try {
      const ruta = await invoke("icon_for_target", { target: destino });
      PREVIA_AUTO = ruta ? urlDeArchivo(ruta) : null;
    } catch {
      // Un destino a medio escribir no es un error que merezca molestar.
      PREVIA_AUTO = null;
    }
    pintarPrevia();
  }, 250);
}

/**
 * Rellena el desplegable de acciones de Windows con los grupos del catalogo.
 *
 * Los `<optgroup>` salen tal cual de Rust: anadir una accion alla la hace
 * aparecer aqui sin tocar este archivo.
 */
function montarCatalogoSistema() {
  $("ed-system-command").innerHTML = CATALOGO_SISTEMA.map(
    (g) =>
      `<optgroup label="${escapar(g.etiqueta)}">` +
      (g.comandos ?? [])
        .map((c) => `<option value="${escapar(c.id)}">${escapar(c.etiqueta)}</option>`)
        .join("") +
      "</optgroup>"
  ).join("");
}

/**
 * Muestra la limitacion real del comando elegido, si la tiene.
 *
 * La grabacion de pantalla depende de la Barra de juegos y no graba el
 * Explorador ni el escritorio. Vale mas decirlo aqui que dejar al usuario
 * pensando que la tecla esta rota.
 */
function mostrarAvisoSistema() {
  const nota = $("ed-system-aviso");
  if (!nota) return;
  const aviso = fichaSistema($("ed-system-command")?.value)?.aviso ?? "";
  nota.textContent = aviso;
  nota.hidden = !aviso;
}

/**
 * Rellena la lista de aplicaciones instaladas.
 *
 * Es la via principal para una tecla de aplicacion: pedirle a alguien la ruta de
 * un .exe es pedirle que sepa donde instala cada fabricante. El campo de ruta
 * sigue debajo para lo que no salga en el menu Inicio.
 */
function montarListaApps(instaladas) {
  const sel = $("ed-app-lista");
  sel.innerHTML =
    '<option value="">— Elige una de tus aplicaciones —</option>' +
    instaladas
      .map((a) => `<option value="${escapar(a.ruta)}">${escapar(a.nombre)}</option>`)
      .join("");
  if (!instaladas.length) {
    $("ed-app-nota").textContent =
      "No se encontró ninguna aplicación en tu menú Inicio. Escribe la ruta abajo.";
  }
}

/**
 * Deja marcada en la lista la aplicacion que ya tuviera la tecla.
 *
 * Si su ruta no esta entre las instaladas (porque se escribio a mano), la lista
 * se queda en la cabecera: la ruta manda, y lo que se ve abajo es la verdad.
 */
function sincronizarListaApps() {
  const sel = $("ed-app-lista");
  if (!sel) return;
  const actual = $("ed-app-target").value.trim();
  const coincide = Array.from(sel.options).some(
    (o) => o.value && o.value.toLowerCase() === actual.toLowerCase()
  );
  sel.value = coincide ? actual : "";
}

/**
 * Deja el formulario utilizable cuando la tecla viene de una version mas nueva.
 *
 * Sin la opcion en el desplegable, el `<select>` se quedaria sin valor y lo
 * siguiente que escribiera el usuario convertiria la tecla en una aplicacion
 * vacia, perdiendo una accion que esta version ni siquiera sabe reconstruir.
 */
function prepararDesconocida(tipo, accion) {
  if (tipo !== "unknown") return;
  const sel = $("ed-tipo");
  if (!Array.from(sel.options).some((o) => o.value === "unknown")) {
    const op = document.createElement("option");
    op.value = "unknown";
    op.textContent = "Acción de una versión más nueva";
    sel.appendChild(op);
  }
  sel.value = "unknown";

  const original = accion?.__original?.type;
  $("ed-unknown-nota").textContent = original
    ? `Esta tecla es de tipo "${original}", que esta versión de MiDeck no conoce. ` +
      "Actualiza para poder usarla."
    : "Esta tecla tiene una acción que esta versión de MiDeck no conoce.";
}

/** Muestra solo el grupo de campos del tipo de accion elegido. */
function mostrarGrupo(tipo) {
  for (const g of document.querySelectorAll("[data-tipo]")) {
    /** @type {HTMLElement} */ (g).hidden = g.getAttribute("data-tipo") !== tipo;
  }
}

function volcarEnFormulario() {
  $("ed-label").value = tecla.label ?? "";
  const tipo = tecla.action?.type ?? "app";
  $("ed-tipo").value = tipo;
  mostrarGrupo(tipo);

  const a = tecla.action ?? {};
  $("ed-app-target").value = tipo === "app" ? a.target ?? "" : "";
  $("ed-app-args").value = tipo === "app" ? a.args ?? "" : "";
  $("ed-app-workdir").value = tipo === "app" ? a.workdir ?? "" : "";
  $("ed-app-focus").checked = tipo === "app" && Boolean(a.focus_if_running);

  $("ed-url-target").value = tipo === "url" ? a.target ?? "" : "";
  $("ed-url-browser").value = tipo === "url" ? a.browser ?? "default" : "default";
  $("ed-url-profile").value = tipo === "url" ? a.profile ?? "" : "";

  $("ed-urls-targets").value = tipo === "urls" ? (a.targets ?? []).join("\n") : "";
  $("ed-urls-browser").value = tipo === "urls" ? a.browser ?? "default" : "default";
  $("ed-urls-profile").value = tipo === "urls" ? a.profile ?? "" : "";
  $("ed-urls-newwindow").checked = tipo === "urls" && Boolean(a.new_window);

  if (tipo === "system" && a.command) $("ed-system-command").value = a.command;
  mostrarAvisoSistema();
  prepararDesconocida(tipo, a);
  sincronizarListaApps();

  $("ed-path-target").value = tipo === "path" ? a.target ?? "" : "";

  $("ed-hotkey-keys").value = tipo === "hotkey" ? a.keys ?? "" : "";
  $("ed-text-texto").value = tipo === "text" ? a.text ?? "" : "";

  $("ed-script-shell").value = tipo === "script" ? a.shell ?? "powershell" : "powershell";
  $("ed-script-target").value = tipo === "script" ? a.target ?? "" : "";
  $("ed-script-args").value = tipo === "script" ? a.args ?? "" : "";
  $("ed-script-hidden").checked = tipo === "script" && Boolean(a.hidden);

  const icon = tecla.icon ?? {};
  $("ed-emoji").value = icon.type === "emoji" ? icon.char ?? "" : "";
  $("ed-fit").value = icon.fit ?? "contain";
  $("ed-labelstyle").value = icon.label_style ?? "below";
  if (icon.background) $("ed-bg").value = icon.background;

  refrescarIconoAuto();
  pintarPrevia();
}

/** Recoge el formulario en el objeto `tecla`. */
function leerFormulario() {
  tecla.label = $("ed-label").value.trim();
  const tipo = $("ed-tipo").value;

  switch (tipo) {
    case "app":
      tecla.action = {
        type: "app",
        target: $("ed-app-target").value.trim(),
        args: $("ed-app-args").value,
        workdir: $("ed-app-workdir").value.trim(),
        focus_if_running: $("ed-app-focus").checked,
      };
      break;
    case "url":
      tecla.action = {
        type: "url",
        target: $("ed-url-target").value.trim(),
        browser: $("ed-url-browser").value,
        profile: $("ed-url-profile").value.trim() || null,
      };
      break;
    case "urls":
      tecla.action = {
        type: "urls",
        // Una direccion por linea. Las vacias se descartan aqui para que el
        // contador de la etiqueta y el limite de Rust cuenten lo mismo.
        targets: $("ed-urls-targets")
          .value.split("\n")
          .map((l) => l.trim())
          .filter(Boolean),
        browser: $("ed-urls-browser").value,
        profile: $("ed-urls-profile").value.trim() || null,
        new_window: $("ed-urls-newwindow").checked,
      };
      break;
    case "system":
      tecla.action = { type: "system", command: $("ed-system-command").value };
      break;
    case "unknown":
      // A proposito no se toca `tecla.action`: sigue siendo el objeto original
      // que mando Rust, asi que guardar la etiqueta o la cara no destruye una
      // accion que esta version no sabe reconstruir.
      break;
    case "path":
      tecla.action = { type: "path", target: $("ed-path-target").value.trim() };
      break;
    case "script":
      tecla.action = {
        type: "script",
        shell: $("ed-script-shell").value,
        target: $("ed-script-target").value.trim(),
        args: $("ed-script-args").value,
        hidden: $("ed-script-hidden").checked,
      };
      break;
    case "folder":
      tecla.action = { type: "folder", surface: $("ed-folder-surface").value };
      break;
    case "hotkey":
      tecla.action = { type: "hotkey", keys: $("ed-hotkey-keys").value.trim() };
      break;
    case "text":
      tecla.action = { type: "text", text: $("ed-text-texto").value };
      break;
    default:
      // Sin esto, un tipo sin `case` guardaba la accion anterior sin avisar.
      throw new Error(`Tipo de acción no contemplado: ${tipo}`);
  }

  const emoji = $("ed-emoji").value.trim();
  if (emoji) {
    tecla.icon = { ...tecla.icon, type: "emoji", char: emoji };
    delete tecla.icon.file;
  } else if (tecla.icon.type === "emoji") {
    tecla.icon = { ...tecla.icon, type: "auto" };
    delete tecla.icon.char;
  }
  tecla.icon.fit = $("ed-fit").value;
  tecla.icon.label_style = $("ed-labelstyle").value;
}

/** Lo que espera Rust: sin los campos auxiliares que solo usa la vista previa. */
function aModelo() {
  const icon = { ...tecla.icon };
  delete icon.src; // solo para pintar aqui
  if (!icon.background) delete icon.background;
  return {
    id: tecla.id,
    position: tecla.position,
    label: tecla.label,
    icon,
    action: tecla.action,
  };
}

// ------------------------------------------------------------------ imagenes

async function elegirImagen() {
  const ruta = await invoke("plugin:dialog|open", {
    options: {
      multiple: false,
      directory: false,
      filters: [
        {
          name: "Imágenes",
          extensions: ["png", "jpg", "jpeg", "webp", "bmp", "ico", "gif", "svg"],
        },
      ],
    },
  });
  if (!ruta) return;
  const archivo = await invoke("import_image", { path: String(ruta) });
  aplicarImagen(archivo);
}

async function pegarImagen() {
  try {
    error("");
    const archivo = await invoke("paste_image_to_library");
    aplicarImagen(archivo);
  } catch (e) {
    error(String(e));
  }
}

function aplicarImagen(archivo) {
  tecla.icon = {
    ...tecla.icon,
    type: "image",
    file: archivo,
    src: urlDeArchivo(rutaBiblioteca(archivo)),
  };
  delete tecla.icon.char;
  $("ed-emoji").value = "";
  pintarPrevia();
}

let CARPETA_BIBLIOTECA = "";
function rutaBiblioteca(archivo) {
  return `${CARPETA_BIBLIOTECA}\\${archivo}`;
}

// ------------------------------------------------------------------- ajustes

async function cargarAjustes(vista) {
  settings = vista.deck.settings;
  $("aj-cols").value = settings.grid.cols;
  $("aj-rows").value = settings.grid.rows;
  $("aj-key").value = settings.key_size;
  $("aj-nivel").value = settings.window_level ?? "top";
  $("aj-hotkey").value = settings.hotkey ?? "";
  $("aj-lock").checked = Boolean(settings.lock_position);
  explicarNivel();
  $("aj-minimized").checked = settings.start_minimized;
  $("aj-capturas-dir").value = settings.screenshot_dir ?? "";
  $("aj-capturas-portapapeles").checked = settings.screenshot_to_clipboard !== false;
  autostartInicial = Boolean(settings.start_with_windows);
  $("aj-autostart").checked = autostartInicial;

  const huerfanas = vista.integrity?.orphans ?? [];
  $("aj-huerfanas").textContent = huerfanas.length
    ? `${huerfanas.length} carpeta(s) ya no son alcanzables desde la raíz. Siguen guardadas con sus teclas.`
    : "No hay carpetas sin usar.";

  await refrescarImagenesSinUso();
}

async function refrescarImagenesSinUso() {
  const sueltas = await invoke("unused_images");
  const total = sueltas.reduce((s, i) => s + i.bytes, 0);
  $("aj-imagenes").textContent = sueltas.length
    ? `${sueltas.length} imagen(es) sin usar, ${(total / 1024).toFixed(0)} KB.`
    : "No hay imágenes sin usar.";
  return sueltas;
}

/** Explica en una linea que implica el nivel elegido. */
function explicarNivel() {
  const nota = $("aj-nivel-nota");
  if (!nota) return;
  nota.textContent = {
    top: "Siempre visible, pero tapa lo que tengas debajo.",
    normal: "Se comporta como cualquier ventana: va y viene del frente.",
    desktop:
      "Vive sobre el fondo de pantalla, por debajo de todo. No estorba nunca; " +
      "lo ves con Win+D o lo traes al frente con el atajo global.",
  }[$("aj-nivel").value] ?? "";
}

async function guardarAjustes() {
  const nuevos = {
    ...settings,
    grid: {
      cols: Math.max(1, Number($("aj-cols").value) || 5),
      rows: Math.max(1, Number($("aj-rows").value) || 3),
    },
    key_size: Math.max(48, Number($("aj-key").value) || 96),
    window_level: $("aj-nivel").value,
    lock_position: $("aj-lock").checked,
    hotkey: $("aj-hotkey").value.trim() || null,
    start_minimized: $("aj-minimized").checked,
    screenshot_dir: $("aj-capturas-dir").value.trim(),
    screenshot_to_clipboard: $("aj-capturas-portapapeles").checked,
  };
  await invoke("update_settings", { settings: nuevos });

  // El autoarranque toca el registro de Windows: solo se llama si cambio.
  if ($("aj-autostart").checked !== autostartInicial) {
    await invoke("set_autostart", { value: $("aj-autostart").checked });
  }
}

// ------------------------------------------------------------------ arranque

async function guardar() {
  error("");
  try {
    if (MODO === "ajustes") {
      await guardarAjustes();
    } else {
      leerFormulario();
      const m = aModelo();
      // Cada tipo tiene su propio campo obligatorio: dar por hecho que todos
      // usan `target` impedia guardar las acciones que no lo tienen.
      const faltante = {
        folder: !m.action.surface && "Elige una carpeta destino.",
        hotkey: !m.action.keys?.trim() && "Escribe la combinación de teclas.",
        text: !m.action.text && "Escribe el texto que debe teclear.",
        urls: !m.action.targets?.length && "Escribe al menos una dirección.",
        system: !m.action.command && "Elige una acción de Windows.",
      }[m.action.type];
      if (faltante) throw new Error(faltante);
      // La lista es de los tipos que SI usan `target`, no de los que no: asi un
      // tipo nuevo no queda bloqueado por olvidarse de excluirlo aquí.
      if (USAN_TARGET.has(m.action.type) && !m.action.target) {
        throw new Error("Falta el destino de la acción.");
      }
      if (!m.label) m.label = etiquetaPorDefecto(m.action);
      await invoke("upsert_button", {
        surfaceId: SURFACE,
        page: PAGINA,
        button: m,
      });
    }
    await invoke("notify_deck_changed");
    await invoke("close_editor");
  } catch (e) {
    error(String(e));
  }
}

/** Tipos de accion cuyo campo obligatorio se llama `target`. */
const USAN_TARGET = new Set(["app", "url", "path", "script"]);

/** Dominio legible de una direccion, para las etiquetas automaticas. */
function dominioDe(url) {
  return ((url ?? "").split("//").pop() ?? "").split("/")[0].replace(/^www\./, "");
}

/** Si el usuario no puso etiqueta, deducir una del destino. */
function etiquetaPorDefecto(action) {
  if (action.type === "hotkey") return action.keys ?? "Atajo";
  if (action.type === "unknown") return "Sin reconocer";
  if (action.type === "system") {
    return fichaSistema(action.command)?.etiqueta ?? "Acción de Windows";
  }
  if (action.type === "urls") {
    const n = action.targets?.length ?? 0;
    if (!n) return "Varias páginas";
    const primera = dominioDe(action.targets[0]);
    return n === 1 ? primera : `${primera} +${n - 1}`;
  }
  if (action.type === "text") {
    const t = (action.text ?? "").trim().split(/\s+/).slice(0, 3).join(" ");
    return t.slice(0, 24) || "Texto";
  }
  const t = action.target ?? "";
  if (action.type === "url") return dominioDe(t);
  const trozos = t.split(/[\\/]/);
  const nombre = trozos[trozos.length - 1] || t;
  return nombre.replace(/\.(exe|lnk|ps1|bat|cmd)$/i, "") || "Sin nombre";
}

/**
 * Captura la siguiente combinacion que pulse el usuario y la escribe en el campo.
 *
 * Escribir "Ctrl+Shift+S" a mano invita a erratas y a dudar de como se llama cada
 * tecla; pulsarla es inequivoco. Se traduce desde `KeyboardEvent.code`, que no
 * depende de la distribucion del teclado.
 */
function capturarAtajo() {
  const campo = $("ed-hotkey-keys");
  const boton = $("ed-hotkey-capturar");
  const textoOriginal = boton.textContent;
  boton.textContent = "Pulsa…";
  boton.disabled = true;
  campo.value = "";

  const nombreDe = (ev) => {
    const c = ev.code;
    if (c.startsWith("Key")) return c.slice(3);
    if (c.startsWith("Digit")) return c.slice(5);
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(c)) return c;
    return {
      Escape: "Esc", Enter: "Intro", NumpadEnter: "Intro", Tab: "Tab",
      Space: "Space", Backspace: "Backspace", Delete: "Supr", Insert: "Ins",
      Home: "Inicio", End: "Fin", PageUp: "PageUp", PageDown: "PageDown",
      ArrowLeft: "Izquierda", ArrowRight: "Derecha", ArrowUp: "Arriba",
      ArrowDown: "Abajo", Period: "Punto", NumpadDecimal: "Punto",
      Comma: "Coma", PrintScreen: "PrtScn",
    }[c] ?? null;
  };

  const terminar = (texto) => {
    window.removeEventListener("keydown", alPulsar, true);
    boton.textContent = textoOriginal;
    boton.disabled = false;
    if (texto) {
      campo.value = texto;
      leerFormulario();
      if (!$("ed-label").value) {
        $("ed-label").value = etiquetaPorDefecto(tecla.action);
        leerFormulario();
      }
      pintarPrevia();
    }
  };

  const alPulsar = (ev) => {
    ev.preventDefault();
    ev.stopPropagation();
    if (ev.key === "Escape" && !ev.ctrlKey && !ev.altKey && !ev.shiftKey && !ev.metaKey) {
      terminar(null); // Escape a secas cancela la captura
      return;
    }
    const base = nombreDe(ev);
    // Mientras solo haya modificadores pulsados no hay nada que capturar.
    if (!base) return;

    const partes = [];
    if (ev.ctrlKey) partes.push("Ctrl");
    if (ev.shiftKey) partes.push("Shift");
    if (ev.altKey) partes.push("Alt");
    if (ev.metaKey) partes.push("Win");
    partes.push(base);
    terminar(partes.join("+"));
  };

  // En captura, para adelantarse a los atajos de la propia ventana.
  window.addEventListener("keydown", alPulsar, true);
}

function conectar() {
  $("ed-tipo").addEventListener("change", () => {
    mostrarGrupo($("ed-tipo").value);
    leerFormulario();
    refrescarIconoAuto();
    pintarPrevia();
  });

  for (const id of [
    "ed-label",
    "ed-emoji",
    "ed-fit",
    "ed-labelstyle",
    "ed-url-target",
    "ed-urls-targets",
    "ed-path-target",
    "ed-script-target",
    "ed-hotkey-keys",
    "ed-text-texto",
  ]) {
    $(id).addEventListener("input", () => {
      leerFormulario();
      refrescarIconoAuto();
      pintarPrevia();
    });
  }

  // Elegir de la lista escribe la ruta en el campo de abajo, no la sustituye: lo
  // que se guarda sigue siendo una ruta, visible y editable a mano.
  $("ed-app-lista").addEventListener("change", (ev) => {
    const ruta = ev.target.value;
    if (!ruta) return;
    $("ed-app-target").value = ruta;
    leerFormulario();
    const nombre = ev.target.options[ev.target.selectedIndex].textContent;
    if (!$("ed-label").value) {
      $("ed-label").value = nombre;
      leerFormulario();
    }
    refrescarIconoAuto();
    pintarPrevia();
  });

  // Y al reves: escribir una ruta a mano deja la lista coherente con ella.
  $("ed-app-target").addEventListener("input", () => {
    sincronizarListaApps();
    leerFormulario();
    refrescarIconoAuto();
    pintarPrevia();
  });

  $("ed-system-command").addEventListener("change", () => {
    mostrarAvisoSistema();
    leerFormulario();
    // La etiqueta de una accion de Windows es su nombre del catalogo: se pone
    // sola mientras el usuario no escriba otra.
    if (!$("ed-label").value) {
      $("ed-label").value = etiquetaPorDefecto(tecla.action);
      leerFormulario();
    }
    pintarPrevia();
  });

  $("ed-bg").addEventListener("input", () => {
    tecla.icon.background = $("ed-bg").value;
    pintarPrevia();
  });
  $("ed-bg-quitar").addEventListener("click", () => {
    delete tecla.icon.background;
    pintarPrevia();
  });

  $("ed-img-elegir").addEventListener("click", () => elegirImagen().catch((e) => error(String(e))));
  $("ed-img-pegar").addEventListener("click", () => pegarImagen());
  $("ed-img-quitar").addEventListener("click", () => {
    tecla.icon = { ...tecla.icon, type: "auto" };
    delete tecla.icon.file;
    delete tecla.icon.src;
    delete tecla.icon.char;
    $("ed-emoji").value = "";
    pintarPrevia();
  });

  // Botones Examinar de los campos de ruta.
  for (const b of document.querySelectorAll("[data-examinar]")) {
    b.addEventListener("click", async () => {
      const destino = b.getAttribute("data-examinar");
      const filtro = b.getAttribute("data-filtro");
      const opciones = { multiple: false, directory: filtro === "dir" };
      if (filtro === "exe") {
        opciones.filters = [{ name: "Programas", extensions: ["exe", "lnk", "bat", "cmd"] }];
      } else if (filtro === "script") {
        opciones.filters = [{ name: "Scripts", extensions: ["ps1", "bat", "cmd"] }];
      }
      try {
        const ruta = await invoke("plugin:dialog|open", { options: opciones });
        if (ruta) {
          $(destino).value = String(ruta);
          sincronizarListaApps();
          leerFormulario();
          refrescarIconoAuto();
          if (!$("ed-label").value) {
            $("ed-label").value = etiquetaPorDefecto(tecla.action);
            leerFormulario();
          }
          pintarPrevia();
        }
      } catch (e) {
        error(String(e));
      }
    });
  }

  $("aj-purgar")?.addEventListener("click", async () => {
    try {
      await invoke("purge_orphans");
      await invoke("notify_deck_changed");
      $("aj-huerfanas").textContent = "Carpetas sin usar eliminadas.";
    } catch (e) {
      error(String(e));
    }
  });

  $("aj-limpiar-img")?.addEventListener("click", async () => {
    try {
      const sueltas = await refrescarImagenesSinUso();
      for (const i of sueltas) await invoke("delete_image", { file: i.file });
      await refrescarImagenesSinUso();
    } catch (e) {
      error(String(e));
    }
  });

  $("aj-nivel")?.addEventListener("change", explicarNivel);

  $("aj-abrir-json")?.addEventListener("click", () =>
    invoke("open_config_file").catch((e) => error(String(e)))
  );

  $("ed-guardar").addEventListener("click", guardar);
  $("ed-cancelar").addEventListener("click", () => invoke("close_editor"));
  document.addEventListener("keydown", (ev) => {
    if (ev.key === "Escape") invoke("close_editor");
    if (ev.key === "Enter" && ev.ctrlKey) guardar();
  });
}

async function iniciar() {
  if (!apiDisponible()) {
    document.body.textContent = "No se encontró la API de Tauri.";
    return;
  }


  const ctx = await invoke("editor_context");
  MODO = ctx.mode ?? "nuevo";
  SURFACE = ctx.surface_id ?? "";
  PAGINA = ctx.page ?? 0;
  POSICION = ctx.position ?? 0;
  BOTON_ID = ctx.button_id ?? null;
  tecla.id = BOTON_ID ?? "";
  tecla.position = POSICION;

  const vista = await invoke("get_deck");
  CARPETA_BIBLIOTECA = vista.config_path.replace(/deck\.json$/i, "icons");

  // Los tokens de rejilla hacen que la vista previa tenga el tamano real.
  document.documentElement.style.setProperty("--key", `${vista.deck.settings.key_size}px`);

  if (MODO === "ajustes") {
    $("vista-ajustes").hidden = false;
    await cargarAjustes(vista);
  } else {
    $("vista-tecla").hidden = false;

    // El catalogo tiene que estar antes de volcar el formulario: de el salen el
    // desplegable, el icono de la vista previa y la etiqueta automatica.
    CATALOGO_SISTEMA = await invoke("list_system_commands");
    setCatalogoSistema(CATALOGO_SISTEMA);
    montarCatalogoSistema();
    montarListaApps(await invoke("list_installed_apps"));

    const superficies = await invoke("list_surfaces");
    $("ed-folder-surface").innerHTML = superficies
      .filter((s) => s.id !== SURFACE)
      .map((s) => `<option value="${escapar(s.id)}">${escapar(s.name)}</option>`)
      .join("");

    if (BOTON_ID) {
      const existente = buscarBoton(vista.deck, BOTON_ID);
      if (existente) {
        tecla = JSON.parse(JSON.stringify(existente));
        const ruta = vista.icon_paths?.[BOTON_ID];
        if (ruta) tecla.icon.src = urlDeArchivo(ruta);
      }
    }
    volcarEnFormulario();
  }

  conectar();
  $("ed-label")?.focus();
}

function buscarBoton(deck, id) {
  for (const s of Object.values(deck.surfaces ?? {})) {
    for (const p of /** @type {any} */ (s).pages ?? []) {
      const b = p.buttons.find((x) => x.id === id);
      if (b) return b;
    }
  }
  return null;
}

iniciar().catch((e) => {
  // Si el fallo ocurrio antes de montar el DOM, no hay donde escribir el error.
  const destino = document.getElementById("ed-error");
  if (destino) error(String(e));
  else document.body.textContent = `MiDeck: ${e}`;
  console.error(e);
});
