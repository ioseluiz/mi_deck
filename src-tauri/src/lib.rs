//! MiDeck: widget de escritorio tipo Stream Deck.
//!
//! La logica vive aqui, en Rust, no en el frontend: modelo, validacion,
//! integridad, imagenes y lanzamiento de procesos son comandos. El frontend solo
//! pinta y envia eventos, asi que lo importante queda cubierto por cargo test.

pub mod edit;
pub mod focus;
pub mod icons;
pub mod images;
pub mod integrity;
pub mod launcher;
pub mod model;
pub mod screen;
pub mod store;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent, Wry,
};
use tauri_plugin_autostart::ManagerExt;

use model::{Action, Deck, IconSource, WindowPos};

pub struct AppState {
    pub deck: Mutex<Deck>,
    pub config_path: PathBuf,
    /// Avisos de la carga inicial (archivo corrupto, respaldo creado...).
    pub warnings: Mutex<Vec<String>>,
}

/// Entradas marcables de la bandeja. Se guardan para que su estado no se
/// desincronice cuando lo mismo se conmuta desde la ventana.
struct TrayItems {
    pin: CheckMenuItem<Wry>,
    inicio: CheckMenuItem<Wry>,
}

/// Lo que el frontend necesita para pintar: el deck, el diagnostico y las rutas de
/// imagen ya resueltas.
#[derive(Serialize)]
pub struct DeckView {
    deck: Deck,
    warnings: Vec<String>,
    integrity: integrity::Report,
    config_path: String,
    /// id de boton -> ruta absoluta de su imagen. El frontend la convierte a URL
    /// con convertFileSrc. Solo trae los botones que tienen imagen resoluble.
    icon_paths: HashMap<String, String>,
}

/// Resultado de pulsar una tecla. Si la accion era de carpeta, el frontend navega.
#[derive(Serialize)]
pub struct ActionOutcome {
    navigate_to: Option<String>,
}

// -------------------------------------------------------- resolucion de iconos

/// Archivo del que sacar el icono automatico de una accion.
///
/// Una URL no tiene archivo: para esas, el frontend dibuja una pastilla con la
/// inicial del dominio. Es deliberado no descargar favicons, para que el widget
/// no dependa jamas de la red ni del proxy institucional.
fn destino_para_icono(action: &Action) -> Option<PathBuf> {
    let crudo = match action {
        Action::App { target, .. } => target,
        Action::Path { target } => target,
        Action::Script { target, .. } => target,
        Action::Url { .. } | Action::Folder { .. } => return None,
    };

    let expandido = launcher::expand_env(crudo);
    if expandido.trim().is_empty() {
        return None;
    }
    launcher::resolve_program(&expandido)
}

/// Resuelve la imagen de cada boton a una ruta absoluta en disco.
///
/// Se hace en el backend, de una vez, en vez de una invocacion por tecla: son
/// quince teclas y todo queda cacheado tras el primer arranque.
fn resolver_iconos(deck: &Deck) -> HashMap<String, String> {
    let mut mapa = HashMap::new();

    for surface in deck.surfaces.values() {
        for page in &surface.pages {
            for button in &page.buttons {
                let ruta = match &button.icon.source {
                    IconSource::Image { file } => {
                        let p = images::resolve(file);
                        p.exists().then_some(p)
                    }
                    IconSource::Auto => {
                        destino_para_icono(&button.action).and_then(|t| icons::shell_icon(&t).ok())
                    }
                    // builtin y emoji los dibuja el frontend.
                    IconSource::Builtin { .. } | IconSource::Emoji { .. } => None,
                };
                if let Some(p) = ruta {
                    mapa.insert(button.id.clone(), p.display().to_string());
                }
            }
        }
    }
    mapa
}

// ------------------------------------------------------------------- comandos

#[tauri::command]
fn get_deck(state: State<AppState>) -> DeckView {
    let deck = state.deck.lock().unwrap();
    DeckView {
        integrity: integrity::check(&deck),
        icon_paths: resolver_iconos(&deck),
        deck: deck.clone(),
        warnings: state.warnings.lock().unwrap().clone(),
        config_path: state.config_path.display().to_string(),
    }
}

/// Relee deck.json del disco y devuelve la vista actualizada.
///
/// Imprescindible mientras no exista el editor visual: se edita el JSON a mano y
/// se recarga sin reiniciar el widget.
#[tauri::command]
fn reload_deck(state: State<AppState>) -> DeckView {
    let cargado = store::load(&state.config_path);
    *state.deck.lock().unwrap() = cargado.deck;
    *state.warnings.lock().unwrap() = cargado.warnings;
    get_deck(state)
}

/// Ejecuta la accion de un boton. Devuelve a donde navegar si era una carpeta.
#[tauri::command]
fn run_action(button_id: String, state: State<AppState>) -> Result<ActionOutcome, String> {
    let accion = {
        let deck = state.deck.lock().unwrap();
        find_action(&deck, &button_id)
            .ok_or_else(|| format!("No existe el boton {button_id} en el deck."))?
    };

    // Si la tecla pide traer al frente lo que ya este abierto, se intenta antes
    // de lanzar: pulsarla diez veces no debe dejar diez copias de la aplicacion.
    if let Action::App {
        target,
        focus_if_running: true,
        ..
    } = &accion
    {
        if let Some(ruta) = launcher::resolve_program(&launcher::expand_env(target)) {
            if focus::focus_running(&ruta) {
                return Ok(ActionOutcome { navigate_to: None });
            }
        }
    }

    let spec = launcher::build_launch(&accion).map_err(|e| e.to_string())?;

    if let launcher::LaunchSpec::Navigate { surface } = &spec {
        // Una referencia rota se detecta aqui, no al pintar: el mensaje debe decir
        // exactamente que carpeta falta.
        let deck = state.deck.lock().unwrap();
        if !deck.surfaces.contains_key(surface) {
            return Err(format!(
                "La carpeta apunta a una superficie que no existe: {surface}"
            ));
        }
        return Ok(ActionOutcome {
            navigate_to: Some(surface.clone()),
        });
    }

    launcher::execute(&spec)?;
    Ok(ActionOutcome { navigate_to: None })
}

/// Monitores conectados, en coordenadas del escritorio virtual.
fn monitores(window: &tauri::Window) -> Vec<screen::Rect> {
    window
        .available_monitors()
        .map(|ms| {
            ms.iter()
                .map(|m| screen::Rect {
                    x: m.position().x,
                    y: m.position().y,
                    w: m.size().width,
                    h: m.size().height,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Tamano actual de la ventana, para decidir si una posicion la deja visible.
fn tam_ventana(window: &tauri::Window) -> (u32, u32) {
    window
        .outer_size()
        .map(|s| (s.width, s.height))
        .unwrap_or((596, 509))
}

/// Guarda la posicion de la ventana. El frontend la llama con freno al mover.
///
/// Minimizar emite un evento de movimiento con la posicion centinela de Windows
/// (-32000, -32000). Guardarla dejaba el widget fuera de toda pantalla al
/// reabrir, corriendo pero inalcanzable: por eso se descarta toda posicion que
/// no quede visible en algun monitor.
#[tauri::command]
fn save_window_pos(
    x: i32,
    y: i32,
    window: tauri::Window,
    state: State<AppState>,
) -> Result<(), String> {
    let pos = WindowPos { x, y };
    if !screen::es_alcanzable(pos, tam_ventana(&window), &monitores(&window)) {
        return Ok(());
    }
    let mut deck = state.deck.lock().unwrap();
    deck.settings.window = Some(pos);
    store::save(&deck, &state.config_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_always_on_top(
    value: bool,
    window: tauri::Window,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    window.set_always_on_top(value).map_err(|e| e.to_string())?;
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.pin.set_checked(value);
    }
    let mut deck = state.deck.lock().unwrap();
    deck.settings.always_on_top = value;
    store::save(&deck, &state.config_path).map_err(|e| e.to_string())
}

/// Direccion del repositorio, sin el sufijo .git para que el navegador la muestre.
pub const REPO_URL: &str = "https://github.com/ioseluiz/mi_deck";

#[tauri::command]
fn open_repo() -> Result<(), String> {
    tauri_plugin_opener::open_url(REPO_URL, None::<&str>)
        .map_err(|e| format!("No se pudo abrir {REPO_URL}: {e}"))
}

#[tauri::command]
fn open_config_file(state: State<AppState>) -> Result<(), String> {
    let path = state.config_path.display().to_string();
    tauri_plugin_opener::open_path(&path, None::<&str>)
        .map_err(|e| format!("No se pudo abrir {path}: {e}"))
}

/// Importa una imagen a la biblioteca y devuelve el nombre a guardar en deck.json.
#[tauri::command]
fn import_image(path: String) -> Result<String, String> {
    images::import(Path::new(&path)).map_err(|e| e.to_string())
}

/// Imagenes de la biblioteca que ninguna tecla referencia.
#[tauri::command]
fn unused_images(state: State<AppState>) -> Vec<images::ImagenSinUso> {
    let deck = state.deck.lock().unwrap();
    images::unused(&deck)
}

/// Borra una imagen. Solo debe llamarse tras confirmacion explicita del usuario.
#[tauri::command]
fn delete_image(file: String) -> Result<(), String> {
    images::delete(&file).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(value: bool, app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let gestor = app.autolaunch();
    if value {
        gestor.enable().map_err(|e| e.to_string())?;
    } else {
        gestor.disable().map_err(|e| e.to_string())?;
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.inicio.set_checked(value);
    }
    let mut deck = state.deck.lock().unwrap();
    deck.settings.start_with_windows = value;
    store::save(&deck, &state.config_path).map_err(|e| e.to_string())
}

fn find_action(deck: &Deck, button_id: &str) -> Option<Action> {
    deck.surfaces.values().find_map(|surface| {
        surface.pages.iter().find_map(|page| {
            page.buttons
                .iter()
                .find(|b| b.id == button_id)
                .map(|b| b.action.clone())
        })
    })
}

// ------------------------------------------------------- comandos de edicion

/// Guarda el deck y devuelve la vista fresca. Todo comando que muta pasa por aqui,
/// asi que no hay ninguna via por la que un cambio quede sin persistir.
fn guardar_y_devolver(state: &State<AppState>) -> Result<DeckView, String> {
    let deck = state.deck.lock().unwrap();
    store::save(&deck, &state.config_path).map_err(|e| e.to_string())?;
    Ok(DeckView {
        integrity: integrity::check(&deck),
        icon_paths: resolver_iconos(&deck),
        deck: deck.clone(),
        warnings: Vec::new(),
        config_path: state.config_path.display().to_string(),
    })
}

/// Aplica una mutacion sobre el deck y persiste. Centralizar el patron evita que
/// alguna operacion futura se olvide de guardar.
fn mutar<F>(state: &State<AppState>, f: F) -> Result<DeckView, String>
where
    F: FnOnce(&mut Deck) -> Result<(), String>,
{
    {
        let mut deck = state.deck.lock().unwrap();
        f(&mut deck)?;
    }
    guardar_y_devolver(state)
}

#[tauri::command]
fn upsert_button(
    surface_id: String,
    page: usize,
    button: model::DeckButton,
    state: State<AppState>,
) -> Result<DeckView, String> {
    mutar(&state, |d| {
        edit::upsert_button(d, &surface_id, page, button)
    })
}

#[tauri::command]
fn delete_button(button_id: String, state: State<AppState>) -> Result<DeckView, String> {
    mutar(&state, |d| edit::delete_button(d, &button_id).map(|_| ()))
}

#[tauri::command]
fn duplicate_button(button_id: String, state: State<AppState>) -> Result<DeckView, String> {
    mutar(&state, |d| {
        edit::duplicate_button(d, &button_id).map(|_| ())
    })
}

#[tauri::command]
fn move_button(
    button_id: String,
    to_surface: String,
    to_page: usize,
    to_position: u32,
    state: State<AppState>,
) -> Result<DeckView, String> {
    mutar(&state, |d| {
        edit::move_button(d, &button_id, &to_surface, to_page, to_position)
    })
}

#[tauri::command]
fn create_folder(
    surface_id: String,
    page: usize,
    position: u32,
    name: String,
    state: State<AppState>,
) -> Result<DeckView, String> {
    mutar(&state, |d| {
        edit::create_folder(d, &surface_id, page, position, &name).map(|_| ())
    })
}

#[tauri::command]
fn rename_surface(
    surface_id: String,
    name: String,
    state: State<AppState>,
) -> Result<DeckView, String> {
    mutar(&state, |d| edit::rename_surface(d, &surface_id, &name))
}

#[tauri::command]
fn add_page(surface_id: String, state: State<AppState>) -> Result<DeckView, String> {
    mutar(&state, |d| edit::add_page(d, &surface_id).map(|_| ()))
}

#[tauri::command]
fn remove_page(
    surface_id: String,
    page: usize,
    state: State<AppState>,
) -> Result<DeckView, String> {
    mutar(&state, |d| edit::remove_page(d, &surface_id, page))
}

/// Borra las superficies que ya no referencia ninguna tecla. Accion explicita del
/// usuario desde Ajustes, nunca automatica.
#[tauri::command]
fn purge_orphans(state: State<AppState>) -> Result<DeckView, String> {
    mutar(&state, |d| {
        edit::purge_orphans(d);
        Ok(())
    })
}

#[tauri::command]
fn update_settings(settings: model::Settings, state: State<AppState>) -> Result<DeckView, String> {
    mutar(&state, |d| {
        d.settings = settings;
        Ok(())
    })
}

/// Cambia la fuente de imagen de una tecla, dejando intactos el ajuste, la
/// etiqueta y la accion: es lo que espera quien suelta un PNG sobre una tecla.
fn poner_imagen(deck: &mut Deck, button_id: &str, archivo: String) -> Result<(), String> {
    let encontrado = deck
        .surfaces
        .values_mut()
        .flat_map(|s| s.pages.iter_mut())
        .flat_map(|p| p.buttons.iter_mut())
        .find(|b| b.id == button_id);
    match encontrado {
        Some(boton) => {
            boton.icon.source = IconSource::Image { file: archivo };
            Ok(())
        }
        None => Err(format!("No existe el boton {button_id}")),
    }
}

#[tauri::command]
fn set_button_image(
    button_id: String,
    path: String,
    state: State<AppState>,
) -> Result<DeckView, String> {
    let archivo = images::import(Path::new(&path)).map_err(|e| e.to_string())?;
    mutar(&state, |d| poner_imagen(d, &button_id, archivo))
}

/// Lee la imagen del portapapeles y la importa a la biblioteca.
///
/// La lectura se hace en Rust y no en el frontend a proposito: el comando
/// `read_image` del plugin devuelve un id de recurso, no bytes, y recomponer la
/// imagen desde JS exigiria encadenar varias llamadas mas. Aqui el plugin entrega
/// RGBA directo y solo hay que empaquetarlo en PNG.
fn pegar_a_biblioteca(app: &AppHandle) -> Result<String, String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let img = app
        .clipboard()
        .read_image()
        .map_err(|_| "El portapapeles no contiene una imagen.".to_string())?;

    let (ancho, alto) = (img.width(), img.height());
    let crudo = img.rgba().to_vec();
    let buffer = image::RgbaImage::from_raw(ancho, alto, crudo)
        .ok_or_else(|| "La imagen del portapapeles tiene un tamano inesperado.".to_string())?;

    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;

    images::import_bytes(&png).map_err(|e| e.to_string())
}

/// Pega la imagen del portapapeles sobre una tecla.
#[tauri::command]
fn paste_image_to_button(
    button_id: String,
    app: AppHandle,
    state: State<AppState>,
) -> Result<DeckView, String> {
    let archivo = pegar_a_biblioteca(&app)?;
    mutar(&state, |d| poner_imagen(d, &button_id, archivo))
}

/// Pega la imagen del portapapeles en la biblioteca y devuelve su nombre, para la
/// tecla que todavia se esta creando en el editor.
#[tauri::command]
fn paste_image_to_library(app: AppHandle) -> Result<String, String> {
    pegar_a_biblioteca(&app)
}

/// Superficie a la que lleva una tecla de carpeta.
///
/// Lo necesita el arrastre: soltar algo sobre una carpeta debe meterlo dentro sin
/// entrar en ella, y para eso hace falta saber a donde apunta.
#[tauri::command]
fn surface_of_button(button_id: String, state: State<AppState>) -> Result<String, String> {
    let deck = state.deck.lock().unwrap();
    match find_action(&deck, &button_id) {
        Some(Action::Folder { surface }) => Ok(surface),
        Some(_) => Err("Esa tecla no es una carpeta.".to_string()),
        None => Err(format!("No existe el boton {button_id}")),
    }
}

/// Que significaria soltar estas rutas aqui, sin tocar nada todavia.
///
/// Se consulta durante el arrastre para resaltar la celda de un color u otro: el
/// resultado de soltar nunca debe ser una sorpresa.
#[tauri::command]
fn preview_drop(paths: Vec<String>, on_button: bool, is_folder_key: bool) -> String {
    let solo_imagenes = !paths.is_empty() && paths.iter().all(|p| images::es_imagen(Path::new(p)));

    if solo_imagenes && on_button {
        "imagen".to_string()
    } else if is_folder_key {
        "dentro".to_string()
    } else if on_button {
        "ocupada".to_string()
    } else {
        "nueva".to_string()
    }
}

/// Crea teclas a partir de rutas soltadas desde el Explorador.
#[tauri::command]
fn drop_paths(
    surface_id: String,
    page: usize,
    position: u32,
    paths: Vec<String>,
    state: State<AppState>,
) -> Result<DeckView, String> {
    mutar(&state, |deck| {
        let mut celda = position;
        for ruta in &paths {
            let p = Path::new(ruta);
            let etiqueta = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| ruta.clone());

            // Una carpeta se abre en el Explorador. Cualquier otra cosa va por
            // `app`, que para lo que no es ejecutable delega en el shell y respeta
            // la aplicacion predeterminada del usuario.
            let action = if p.is_dir() {
                Action::Path {
                    target: ruta.clone(),
                }
            } else {
                Action::App {
                    target: ruta.clone(),
                    args: String::new(),
                    workdir: String::new(),
                    focus_if_running: false,
                }
            };

            let boton = model::DeckButton {
                id: edit::nuevo_id(deck, "b"),
                position: celda,
                label: etiqueta,
                // Una imagen soltada se convierte en la cara de su propia tecla.
                icon: images::icono_para(p),
                action,
            };
            edit::upsert_button(deck, &surface_id, page, boton)?;
            celda = celda.saturating_add(1);
        }
        Ok(())
    })
}

/// Crea una tecla de URL a partir de texto soltado desde el navegador.
#[tauri::command]
fn drop_url(
    surface_id: String,
    page: usize,
    position: u32,
    url: String,
    state: State<AppState>,
) -> Result<DeckView, String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("No se solto ninguna direccion.".to_string());
    }
    mutar(&state, |deck| {
        let etiqueta = dominio_corto(&url);
        let boton = model::DeckButton {
            id: edit::nuevo_id(deck, "b"),
            position,
            label: etiqueta,
            icon: model::Icon::default(),
            action: Action::Url {
                target: url.clone(),
                browser: "default".to_string(),
                profile: None,
            },
        };
        edit::upsert_button(deck, &surface_id, page, boton)
    })
}

/// Etiqueta razonable para una URL: el dominio sin esquema ni www.
fn dominio_corto(url: &str) -> String {
    let sin_esquema = url.split("://").last().unwrap_or(url);
    let host = sin_esquema.split('/').next().unwrap_or(sin_esquema);
    host.trim_start_matches("www.").to_string()
}

// ------------------------------------------------------- ventana del editor

/// Contexto de la ventana del editor: que se esta editando y donde va a parar.
///
/// Viaja por estado y no por la URL: evita codificar y parsear parametros, y deja
/// el contexto tipado a ambos lados.
#[derive(Clone, Default, Serialize)]
pub struct EditorCtx {
    mode: String,
    button_id: Option<String>,
    surface_id: String,
    page: usize,
    position: u32,
}

#[derive(Default)]
pub struct EditorState(Mutex<EditorCtx>);

/// Abre el editor en una ventana propia.
///
/// El panel mide 596 x 479 y no es redimensionable: un formulario con todos los
/// campos de una accion mas la vista previa no cabe ahi. La ventana se crea bajo
/// demanda y se destruye al cerrar, para no cargar un segundo webview en memoria
/// mientras no se usa.
/// Es `async` por necesidad, no por estilo: construir una ventana de forma
/// sincrona dentro de un comando bloquea el hilo principal mientras ese mismo
/// hilo tiene que atender la creacion del webview. El resultado es una ventana
/// con marco pero sin contenido, sin ningun error. Un comando async se ejecuta
/// fuera de ese hilo y no se traba.
#[tauri::command]
async fn open_editor(
    app: AppHandle,
    mode: String,
    button_id: Option<String>,
    surface_id: String,
    page: usize,
    position: u32,
) -> Result<(), String> {
    *app.state::<EditorState>().0.lock().unwrap() = EditorCtx {
        mode: mode.clone(),
        button_id,
        surface_id,
        page,
        position,
    };

    // Si ya habia un editor abierto, se destruye: recrearlo es mas simple y mas
    // seguro que intentar reajustar su contexto en caliente.
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.destroy();
    }

    let titulo = match mode.as_str() {
        "ajustes" => "MiDeck - Ajustes",
        "nuevo" => "MiDeck - Nueva tecla",
        _ => "MiDeck - Editar tecla",
    };
    let (ancho, alto) = if mode == "ajustes" {
        (520.0, 600.0)
    } else {
        (800.0, 620.0)
    };

    tauri::WebviewWindowBuilder::new(&app, "editor", tauri::WebviewUrl::App("editor.html".into()))
        .title(titulo)
        .inner_size(ancho, alto)
        .min_inner_size(460.0, 420.0)
        .resizable(true)
        .center()
        // Encima del panel, que a su vez suele estar encima de todo.
        .always_on_top(true)
        .build()
        .map_err(|e| e.to_string())?;

    // Las herramientas del webview a mano cuando hagan falta: si una ventana sale
    // en blanco, el motivo esta en su consola y en ningun otro sitio.
    #[cfg(debug_assertions)]
    if std::env::var("MIDECK_DEVTOOLS").is_ok() {
        if let Some(w) = app.get_webview_window("editor") {
            w.open_devtools();
        }
    }

    Ok(())
}

/// Lo que el editor necesita saber nada mas cargar.
#[tauri::command]
fn editor_context(state: State<EditorState>) -> EditorCtx {
    state.0.lock().unwrap().clone()
}

/// Lista de superficies para el desplegable de carpetas del editor.
#[derive(Serialize)]
pub struct SurfaceInfo {
    id: String,
    name: String,
}

#[tauri::command]
fn list_surfaces(state: State<AppState>) -> Vec<SurfaceInfo> {
    let deck = state.deck.lock().unwrap();
    let mut v: Vec<SurfaceInfo> = deck
        .surfaces
        .iter()
        .map(|(id, s)| SurfaceInfo {
            id: id.clone(),
            name: s.name.clone(),
        })
        .collect();
    v.sort_by_key(|s| s.name.to_lowercase());
    v
}

/// Avisa a todas las ventanas de que el deck cambio, para que se repinten.
#[tauri::command]
fn notify_deck_changed(app: AppHandle) {
    let _ = app.emit("deck-changed", ());
}

/// Cierra la ventana del editor.
#[tauri::command]
fn close_editor(app: AppHandle) {
    if let Some(w) = app.get_webview_window("editor") {
        let _ = w.destroy();
    }
}

// -------------------------------------------------------------------- ventana

fn alternar_ventana(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn guardar_posicion(window: &tauri::Window) {
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let pos = WindowPos { x: pos.x, y: pos.y };
    // Misma salvaguarda que en save_window_pos: una ventana minimizada reporta
    // una posicion que no sirve para restaurar nada.
    if !screen::es_alcanzable(pos, tam_ventana(window), &monitores(window)) {
        return;
    }
    let state = window.state::<AppState>();
    let mut deck = state.deck.lock().unwrap();
    deck.settings.window = Some(pos);
    let _ = store::save(&deck, &state.config_path);
}

// --------------------------------------------------------------------- bandeja

fn construir_bandeja(app: &AppHandle, always_on_top: bool) -> tauri::Result<()> {
    let autostart_activo = app.autolaunch().is_enabled().unwrap_or(false);

    let mostrar = MenuItem::with_id(app, "mostrar", "Mostrar u ocultar", true, None::<&str>)?;
    let pin = CheckMenuItem::with_id(
        app,
        "pin",
        "Siempre encima",
        true,
        always_on_top,
        None::<&str>,
    )?;
    let config = MenuItem::with_id(app, "config", "Abrir deck.json", true, None::<&str>)?;
    let inicio = CheckMenuItem::with_id(
        app,
        "inicio",
        "Iniciar con Windows",
        true,
        autostart_activo,
        None::<&str>,
    )?;
    let separador = PredefinedMenuItem::separator(app)?;
    let salir = MenuItem::with_id(app, "salir", "Salir", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&mostrar, &pin, &config, &inicio, &separador, &salir])?;

    app.manage(TrayItems {
        pin: pin.clone(),
        inicio: inicio.clone(),
    });

    TrayIconBuilder::with_id("principal")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("MiDeck")
        .menu(&menu)
        // El clic izquierdo alterna la ventana; el menu va en el derecho, que es
        // lo que espera cualquiera en Windows.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(w) = tray.app_handle().get_webview_window("main") {
                    alternar_ventana(&w);
                }
            }
        })
        .on_menu_event(|app, event| {
            let ventana = app.get_webview_window("main");
            match event.id.as_ref() {
                "mostrar" => {
                    if let Some(w) = ventana {
                        alternar_ventana(&w);
                    }
                }
                "pin" => {
                    let items = app.state::<TrayItems>();
                    let nuevo = items.pin.is_checked().unwrap_or(false);
                    if let Some(w) = ventana {
                        let _ = w.set_always_on_top(nuevo);
                    }
                    let state = app.state::<AppState>();
                    let mut deck = state.deck.lock().unwrap();
                    deck.settings.always_on_top = nuevo;
                    let _ = store::save(&deck, &state.config_path);
                }
                "config" => {
                    let state = app.state::<AppState>();
                    let _ = tauri_plugin_opener::open_path(
                        state.config_path.display().to_string(),
                        None::<&str>,
                    );
                }
                "inicio" => {
                    let items = app.state::<TrayItems>();
                    let nuevo = items.inicio.is_checked().unwrap_or(false);
                    let gestor = app.autolaunch();
                    let r = if nuevo {
                        gestor.enable()
                    } else {
                        gestor.disable()
                    };
                    if r.is_err() {
                        // Si Windows rechazo el cambio, la marca debe reflejarlo.
                        let _ = items.inicio.set_checked(!nuevo);
                    } else {
                        let state = app.state::<AppState>();
                        let mut deck = state.deck.lock().unwrap();
                        deck.settings.start_with_windows = nuevo;
                        let _ = store::save(&deck, &state.config_path);
                    }
                }
                "salir" => {
                    if let Some(w) = app.get_webview_window("main") {
                        guardar_posicion(&w.as_ref().window());
                    }
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

// -------------------------------------------------------------------- arranque

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = store::config_path();
    let cargado = store::load(&config_path);

    // Si el archivo no existia, dejarlo escrito ya: asi el usuario tiene algo
    // concreto que abrir y editar desde el primer arranque.
    if !config_path.exists() {
        let _ = store::save(&cargado.deck, &config_path);
    }

    let always_on_top = cargado.deck.settings.always_on_top;
    let start_minimized = cargado.deck.settings.start_minimized;
    let posicion = cargado.deck.settings.window;

    let estado = AppState {
        deck: Mutex::new(cargado.deck),
        config_path,
        warnings: Mutex::new(cargado.warnings),
    };

    let mut builder = tauri::Builder::default();

    // El plugin de instancia unica tiene que registrarse el primero.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(estado)
        .manage(EditorState::default())
        .invoke_handler(tauri::generate_handler![
            get_deck,
            reload_deck,
            run_action,
            save_window_pos,
            set_always_on_top,
            set_autostart,
            open_config_file,
            open_repo,
            import_image,
            unused_images,
            delete_image,
            upsert_button,
            delete_button,
            duplicate_button,
            move_button,
            create_folder,
            rename_surface,
            add_page,
            remove_page,
            purge_orphans,
            update_settings,
            set_button_image,
            paste_image_to_button,
            paste_image_to_library,
            preview_drop,
            surface_of_button,
            drop_paths,
            drop_url,
            open_editor,
            editor_context,
            close_editor,
            list_surfaces,
            notify_deck_changed
        ])
        .setup(move |app| {
            // El protocolo asset: solo sirve lo que se le permite explicitamente.
            // Sin esto las teclas saldrian sin imagen y sin ningun error visible.
            let scope = app.asset_protocol_scope();
            let _ = scope.allow_directory(images::library_dir(), true);
            let _ = scope.allow_directory(icons::cache_dir(), true);

            construir_bandeja(app.handle(), always_on_top)?;

            let window = app.get_webview_window("main").expect("ventana principal");

            // La ventana nace oculta: se coloca primero y se muestra despues, para
            // que no parpadee en la esquina equivocada al arrancar.
            //
            // La posicion guardada puede haber dejado de valer, por ejemplo si se
            // desconecto el monitor donde estaba. Si no queda visible, se descarta
            // y la ventana se queda centrada.
            let ventana = window.as_ref().window();
            let segura =
                screen::posicion_segura(posicion, tam_ventana(&ventana), &monitores(&ventana));
            if let Some(WindowPos { x, y }) = segura {
                let _ = window.set_position(PhysicalPosition::new(x, y));
            }
            let _ = window.set_always_on_top(always_on_top);

            if !start_minimized {
                let _ = window.show();
                let _ = window.set_focus();
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Solo el panel se oculta a la bandeja: es un widget, no una
                // ventana de documento. El editor se cierra de verdad, o quedaria
                // invisible ocupando memoria y la proxima apertura lo recrearia.
                if window.label() == "main" {
                    api.prevent_close();
                    guardar_posicion(window);
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error al arrancar MiDeck");
}
