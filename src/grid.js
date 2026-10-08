// @ts-check
/**
 * Pintado de la rejilla de teclas.
 *
 * La tecla de volver no vive en deck.json: la inyecta este render en la celda 0
 * de toda superficie que no sea la raiz, igual que el Stream Deck fisico. Por eso
 * la raiz tiene 15 teclas utiles y una carpeta 14.
 */

export const ICONOS = {
  folder:
    '<svg viewBox="0 0 24 24"><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
  "folder-open":
    '<svg viewBox="0 0 24 24"><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v1H3zM3 10h18l-2 7a2 2 0 0 1-2 1.5H5A2 2 0 0 1 3 17z"/></svg>',
  back: '<svg viewBox="0 0 24 24"><path d="M11 5 4 12l7 7v-4h5a3 3 0 0 1 3 3v2h2v-2a5 5 0 0 0-5-5h-5z"/></svg>',
  globe:
    '<svg viewBox="0 0 24 24"><path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20m0 2c1.3 0 2.7 2.2 3.2 5.3H8.8C9.3 6.2 10.7 4 12 4M6.8 9.3H4.3A8 8 0 0 1 9 4.6c-1 1.2-1.8 2.8-2.2 4.7m-2.6 2h2.3c-.1 1.1-.1 2.3 0 3.4H4.2a8 8 0 0 1 0-3.4m.1 5.4h2.5c.4 1.9 1.1 3.5 2.2 4.7a8 8 0 0 1-4.7-4.7M12 20c-1.3 0-2.7-2.2-3.2-5.3h6.4C14.7 17.8 13.3 20 12 20m3.5-7.3h-7a23 23 0 0 1 0-3.4h7a23 23 0 0 1 0 3.4m-.4 8.7c1-1.2 1.8-2.8 2.2-4.7h2.5a8 8 0 0 1-4.7 4.7m2.5-6.7c.1-1.1.1-2.3 0-3.4h2.3a8 8 0 0 1 0 3.4zm-.3-5.4c-.4-1.9-1.2-3.5-2.2-4.7a8 8 0 0 1 4.7 4.7z"/></svg>',
  terminal:
    '<svg viewBox="0 0 24 24"><path d="M3 4h18a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1m3.3 4.3-1.4 1.4L7.6 12l-2.7 2.3 1.4 1.4L10.4 12zM12 15h6v2h-6z"/></svg>',
  app: '<svg viewBox="0 0 24 24"><path d="M4 4h7v7H4zm9 0h7v7h-7zM4 13h7v7H4zm9 0h7v7h-7z"/></svg>',
  file: '<svg viewBox="0 0 24 24"><path d="M6 2h7l5 5v15H6zm7 1.5V8h4.5z"/></svg>',
  aviso:
    '<svg viewBox="0 0 24 24"><path d="M12 2 1 21h22zm0 6 7.5 11h-15zM11 11h2v5h-2zm0 6h2v2h-2z"/></svg>',
  keyboard:
    '<svg viewBox="0 0 24 24"><path d="M3 5h18a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1m2 3v2h2V8zm4 0v2h2V8zm4 0v2h2V8zm4 0v2h2V8zM5 12v2h2v-2zm4 0v2h2v-2zm4 0v2h2v-2zm4 0v2h2v-2zM7 16v2h10v-2z"/></svg>',
  text: '<svg viewBox="0 0 24 24"><path d="M4 4h16v4h-2V6h-5v12h2v2H9v-2h2V6H6v2H4z"/></svg>',
};

/** Icono por defecto segun el tipo de accion, cuando la tecla no define uno. */
function iconoPorAccion(accion) {
  switch (accion?.type) {
    case "folder":
      return ICONOS.folder;
    case "url":
      return ICONOS.globe;
    case "script":
      return ICONOS.terminal;
    case "path":
      return ICONOS["folder-open"];
    case "hotkey":
      return ICONOS.keyboard;
    case "text":
      return ICONOS.text;
    case "app":
    default:
      return ICONOS.app;
  }
}

/**
 * Dominio de una URL, para la pastilla de inicial.
 * @param {string} url
 */
function dominioDe(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return String(url).replace(/^[a-z]+:\/\//i, "").split("/")[0];
  }
}

/**
 * Pastilla con la inicial del dominio sobre un color derivado del propio dominio.
 *
 * Se usa en vez de descargar el favicon a proposito: asi el widget no toca la red
 * jamas, lo que lo hace inmune al proxy institucional y a trabajar sin conexion.
 * El tono sale del dominio, de modo que un mismo sitio siempre tiene el mismo
 * color y se reconoce de un vistazo.
 *
 * @param {string} url
 */
function pastilla(url) {
  const dominio = dominioDe(url) || "?";
  const letra = escapar(dominio.charAt(0).toUpperCase() || "?");
  let h = 0;
  for (const c of dominio) h = (h * 31 + c.charCodeAt(0)) % 360;
  return `<span class="pastilla" style="background:hsl(${h} 48% 40%)">${letra}</span>`;
}

/**
 * Contenido visual de la tecla segun la fuente del icono.
 *
 * Las imagenes (de la biblioteca o extraidas del .exe) llegan ya resueltas por
 * Rust en `urls`; aqui solo se eligen.
 *
 * @param {any} boton
 * @param {Record<string, string>} urls
 */
function pintarIcono(boton, urls) {
  const icon = boton.icon ?? { type: "auto" };
  const url = urls[boton.id];

  if (icon.type === "emoji") return escapar(icon.char ?? "");
  if (icon.type === "builtin") {
    return ICONOS[icon.name] ?? iconoPorAccion(boton.action);
  }
  if (url) {
    return `<img src="${escapar(url)}" alt="" draggable="false">`;
  }
  // Sin archivo del que sacar icono: una URL recibe su pastilla y el resto,
  // el generico de su tipo de accion.
  if (boton.action?.type === "url") return pastilla(boton.action.target ?? "");
  return iconoPorAccion(boton.action);
}

function escapar(s) {
  return String(s).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]
  );
}

/**
 * Coloca los botones en las celdas respetando `position`, pero sin perder
 * ninguno: si dos botones piden la misma celda, o la celda 0 esta ocupada por la
 * tecla de volver, el sobrante cae en el primer hueco libre.
 *
 * @param {any[]} botones
 * @param {number} celdas
 * @param {boolean} reservarCeldaCero
 */
export function distribuir(botones, celdas, reservarCeldaCero) {
  const rejilla = new Array(celdas).fill(null);
  const pendientes = [];

  for (const b of botones) {
    const pos = Number(b.position);
    const valida =
      Number.isInteger(pos) &&
      pos >= 0 &&
      pos < celdas &&
      !(reservarCeldaCero && pos === 0) &&
      rejilla[pos] === null;
    if (valida) rejilla[pos] = b;
    else pendientes.push(b);
  }

  for (const b of pendientes) {
    const libre = rejilla.findIndex(
      (c, i) => c === null && !(reservarCeldaCero && i === 0)
    );
    if (libre !== -1) rejilla[libre] = b;
  }

  return rejilla;
}

/**
 * Pinta la rejilla completa.
 * @param {HTMLElement} contenedor
 * @param {import("./nav.js").Nav} nav
 * @param {{ rotas: Set<string>, urls: Record<string, string> }} ctx
 */
export function render(contenedor, nav, ctx) {
  const grid = nav.deck.settings.grid;
  const celdas = grid.cols * grid.rows;
  const hayVolver = nav.depth > 0;
  const rejilla = distribuir(nav.buttons, celdas, hayVolver);

  const partes = [];

  if (hayVolver) {
    partes.push(
      `<button class="key key--volver" data-accion="volver" title="Volver a ${escapar(
        nav.deck.surfaces[nav.stack[nav.stack.length - 2]]?.name ?? ""
      )}">` +
        `<span class="key-icono">${ICONOS.back}</span>` +
        `<span class="key-etiqueta">Volver</span>` +
        `</button>`
    );
  }

  for (let i = hayVolver ? 1 : 0; i < celdas; i++) {
    const b = rejilla[i];
    if (!b) {
      partes.push(`<div class="key key--vacia" data-celda="${i}"></div>`);
      continue;
    }

    const icon = b.icon ?? {};
    const clases = ["key"];
    if (b.action?.type === "folder") clases.push("key--carpeta");
    if (icon.fit === "cover") clases.push("key--cover");
    if (icon.label_style === "overlay") clases.push("key--label-overlay");
    if (icon.label_style === "none") clases.push("key--label-none");

    const rota = ctx.rotas.has(b.id);
    if (rota) clases.push("key--error");

    const fondo = icon.background
      ? ` style="background:${escapar(icon.background)}"`
      : "";

    partes.push(
      `<button class="${clases.join(" ")}" data-boton="${escapar(b.id)}" ` +
        `data-indice="${i}"${fondo} ` +
        `title="${escapar(b.label || "")}">` +
        `<span class="key-icono">${rota ? ICONOS.aviso : pintarIcono(b, ctx.urls)}</span>` +
        `<span class="key-etiqueta">${escapar(b.label || "")}</span>` +
        `</button>`
    );
  }

  contenedor.innerHTML = partes.join("");
}

/**
 * Pinta las migas de pan.
 * @param {HTMLElement} contenedor
 * @param {import("./nav.js").Nav} nav
 */
export function renderMigas(contenedor, nav) {
  const trail = nav.trail();
  contenedor.innerHTML = trail
    .map((m, i) => {
      const sep = i > 0 ? '<span class="crumb-sep">&rsaquo;</span>' : "";
      const clase = m.actual ? "crumb crumb--actual" : "crumb";
      const attrs = m.actual ? "disabled" : `data-nivel="${m.index}"`;
      return `${sep}<button class="${clase}" ${attrs}>${escapar(m.name)}</button>`;
    })
    .join("");
}
