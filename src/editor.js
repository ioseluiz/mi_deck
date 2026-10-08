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
  } else if (tecla.action?.type === "url") {
    const dom = (tecla.action.target || "").split("//").pop()?.split("/")[0] ?? "?";
    let h = 0;
    for (const c of dom) h = (h * 31 + c.charCodeAt(0)) % 360;
    contenido = `<span class="pastilla" style="background:hsl(${h} 48% 40%)">${escapar(
      (dom[0] || "?").toUpperCase()
    )}</span>`;
  } else {
    contenido = '<svg viewBox="0 0 24 24"><path d="M4 4h7v7H4zm9 0h7v7h-7zM4 13h7v7H4zm9 0h7v7h-7z"/></svg>';
  }

  prev.innerHTML =
    `<span class="key-icono">${contenido}</span>` +
    `<span class="key-etiqueta">${escapar(tecla.label || "")}</span>`;
}

// ------------------------------------------------------- formulario de tecla

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

  $("ed-path-target").value = tipo === "path" ? a.target ?? "" : "";

  $("ed-script-shell").value = tipo === "script" ? a.shell ?? "powershell" : "powershell";
  $("ed-script-target").value = tipo === "script" ? a.target ?? "" : "";
  $("ed-script-args").value = tipo === "script" ? a.args ?? "" : "";
  $("ed-script-hidden").checked = tipo === "script" && Boolean(a.hidden);

  const icon = tecla.icon ?? {};
  $("ed-emoji").value = icon.type === "emoji" ? icon.char ?? "" : "";
  $("ed-fit").value = icon.fit ?? "contain";
  $("ed-labelstyle").value = icon.label_style ?? "below";
  if (icon.background) $("ed-bg").value = icon.background;

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
  $("aj-ontop").checked = settings.always_on_top;
  $("aj-minimized").checked = settings.start_minimized;
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

async function guardarAjustes() {
  const nuevos = {
    ...settings,
    grid: {
      cols: Math.max(1, Number($("aj-cols").value) || 5),
      rows: Math.max(1, Number($("aj-rows").value) || 3),
    },
    key_size: Math.max(48, Number($("aj-key").value) || 96),
    always_on_top: $("aj-ontop").checked,
    start_minimized: $("aj-minimized").checked,
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
      if (m.action.type === "folder" && !m.action.surface) {
        throw new Error("Elige una carpeta destino.");
      }
      if (m.action.type !== "folder" && !m.action.target) {
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

/** Si el usuario no puso etiqueta, deducir una del destino. */
function etiquetaPorDefecto(action) {
  const t = action.target ?? "";
  if (action.type === "url") {
    return (t.split("//").pop() ?? t).split("/")[0].replace(/^www\./, "");
  }
  const trozos = t.split(/[\\/]/);
  const nombre = trozos[trozos.length - 1] || t;
  return nombre.replace(/\.(exe|lnk|ps1|bat|cmd)$/i, "") || "Sin nombre";
}

function conectar() {
  $("ed-tipo").addEventListener("change", () => {
    mostrarGrupo($("ed-tipo").value);
    leerFormulario();
    pintarPrevia();
  });

  for (const id of [
    "ed-label",
    "ed-emoji",
    "ed-fit",
    "ed-labelstyle",
    "ed-url-target",
  ]) {
    $(id).addEventListener("input", () => {
      leerFormulario();
      pintarPrevia();
    });
  }

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
          leerFormulario();
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
