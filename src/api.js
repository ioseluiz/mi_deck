// @ts-check
/**
 * Envoltura del global window.__TAURI__.
 *
 * El proyecto no usa bundler ni node_modules: la API llega por el global que
 * habilita `app.withGlobalTauri` en tauri.conf.json, en vez del paquete npm
 * @tauri-apps/api. Todo el acceso al global pasa por aqui, asi que si algun dia
 * hiciera falta volver a Vite solo cambia este archivo.
 */

const T = /** @type {any} */ (window).__TAURI__;

/**
 * Nombres de los modulos disponibles en el global. Util para diagnosticar de un
 * vistazo si withGlobalTauri expuso lo que se espera.
 * @returns {string[]}
 */
export function modulosDisponibles() {
  return T ? Object.keys(T).sort() : [];
}

/** @returns {boolean} */
export function apiDisponible() {
  return Boolean(T && T.core && typeof T.core.invoke === "function");
}

/**
 * Llama a un comando de Rust.
 * @param {string} comando
 * @param {Record<string, unknown>} [args]
 * @returns {Promise<any>}
 */
export function invoke(comando, args) {
  if (!apiDisponible()) {
    return Promise.reject(
      new Error(
        "La API de Tauri no esta disponible en window.__TAURI__. " +
          "Revisa que app.withGlobalTauri siga en true en tauri.conf.json."
      )
    );
  }
  return T.core.invoke(comando, args);
}

/** Ventana actual, para minimizar, cerrar o fijar encima. */
export function ventana() {
  return T.window.getCurrentWindow();
}

/**
 * Convierte una ruta absoluta en una URL que el webview puede cargar.
 *
 * Solo funciona para carpetas permitidas en el scope del protocolo asset:, que
 * Rust habilita al arrancar (biblioteca de imagenes y cache de iconos). Fuera de
 * ahi la imagen no carga y no se emite ningun error: por eso el scope se declara
 * explicitamente en lugar de confiar en que funcione.
 *
 * @param {string} ruta
 * @returns {string}
 */
export function urlDeArchivo(ruta) {
  return T.core.convertFileSrc(ruta);
}
