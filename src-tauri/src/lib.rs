//! MiDeck: widget de escritorio tipo Stream Deck.
//!
//! La logica vive aqui, en Rust, no en el frontend: modelo, validacion,
//! integridad, imagenes y lanzamiento de procesos son comandos. El frontend solo
//! pinta y envia eventos, asi que lo importante queda cubierto por cargo test.

pub mod apps;
pub mod burbuja;
pub mod captura;
pub mod edit;
pub mod explorador;
pub mod focus;
pub mod icons;
pub mod images;
pub mod integrity;
pub mod launcher;
pub mod model;
pub mod nivel;
pub mod perfiles;
pub mod screen;
pub mod sistema;
pub mod store;
pub mod teclas;

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

use model::{Action, Deck, IconSource, WindowLevel, WindowPos};

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
    /// Botones que exigen pulsar dos veces antes de ejecutarse.
    ///
    /// Quien decide que es peligroso es el catalogo de Rust, no el frontend: asi
    /// una accion destructiva nueva queda protegida por el hecho de declararla,
    /// sin que haya que acordarse de tocar tambien el JavaScript.
    confirm_required: Vec<String>,
}

/// Botones del deck cuya accion esta marcada como peligrosa en el catalogo.
fn botones_a_confirmar(deck: &Deck) -> Vec<String> {
    let mut v = Vec::new();
    for surface in deck.surfaces.values() {
        for page in &surface.pages {
            for button in &page.buttons {
                if let Action::System { command } = &button.action {
                    if sistema::info(*command).is_some_and(|f| f.peligroso) {
                        v.push(button.id.clone());
                    }
                }
            }
        }
    }
    v.sort();
    v
}

/// Resultado de pulsar una tecla. Si la accion era de carpeta, el frontend navega.
#[derive(Serialize)]
pub struct ActionOutcome {
    navigate_to: Option<String>,
    /// Aviso que mostrar al usuario. Una captura que no dice donde quedo el
    /// archivo es una captura que el usuario no encuentra.
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
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
        // Estas no tienen archivo del que sacar un icono: lo pone el frontend.
        Action::Url { .. }
        | Action::Urls { .. }
        | Action::Folder { .. }
        | Action::Hotkey { .. }
        | Action::Text { .. }
        | Action::System { .. }
        // Una macro no tiene un destino: su icono lo pone el frontend.
        | Action::Macro { .. }
        | Action::Unknown { .. } => return None,
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
        confirm_required: botones_a_confirmar(&deck),
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
async fn reload_deck(app: AppHandle, state: State<'_, AppState>) -> Result<DeckView, String> {
    let cargado = store::load(&state.config_path);
    let encendida = cargado.deck.settings.bubble;
    let esquina = cargado.deck.settings.bubble_corner;
    *state.deck.lock().unwrap() = cargado.deck;
    *state.warnings.lock().unwrap() = cargado.warnings;

    // Recargar es la via de quien edita deck.json a mano: la burbuja tiene que
    // obedecer a lo que ponga ahi igual que todo lo demas.
    let _ = burbuja::aplicar(&app, encendida, esquina);
    Ok(get_deck(state))
}

/// Ejecuta la accion de un boton. Devuelve a donde navegar si era una carpeta.
#[tauri::command]
fn run_action(
    app: AppHandle,
    button_id: String,
    state: State<AppState>,
) -> Result<ActionOutcome, String> {
    let accion = {
        let deck = state.deck.lock().unwrap();
        find_action(&deck, &button_id)
            .ok_or_else(|| format!("No existe el boton {button_id} en el deck."))?
    };

    // %CARPETA% se resuelve aqui y no dentro de `expand_env`, que es pura y esta
    // cubierta por tests: meterle una llamada a COM la dejaria sin poder probarse.
    // La consulta solo ocurre si la tecla menciona la variable.
    let accion = explorador::con_variables(accion, explorador::carpeta_disponible)?;

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
            message: None,
        });
    }

    let message = ejecutar_spec(&app, &state, &spec)?;
    Ok(ActionOutcome {
        navigate_to: None,
        message,
    })
}

/// Ejecuta un lanzamiento, incluidas las variantes que `launcher::execute` no
/// puede resolver por si solo.
///
/// La captura necesita los ajustes del usuario y el portapapeles; la secuencia
/// necesita poder ejecutar cada paso, incluidos los que tambien son captura. Al
/// tener un solo sitio que sabe ejecutar cualquier cosa, **un paso de macro puede
/// ser cualquier accion de MiDeck sin ningun caso especial**.
///
/// Devuelve el aviso que haya que mostrar, si lo hay.
fn ejecutar_spec(
    app: &AppHandle,
    state: &State<AppState>,
    spec: &launcher::LaunchSpec,
) -> Result<Option<String>, String> {
    match spec {
        launcher::LaunchSpec::Capture { objetivo } => hacer_captura(app, state, *objetivo)
            .map(|destino| Some(format!("Captura guardada en {destino}"))),

        launcher::LaunchSpec::Secuencia(pasos) => {
            // La secuencia entera se valida ANTES de ejecutar el primer paso.
            // Hace falta explicitamente: `execute` valida solo el spec que
            // recibe, asi que sin esto una errata en el paso tres se descubriria
            // con los dos primeros ya ejecutados, y una macro a medias es peor
            // que una macro que no arranca. Lo encontro un test.
            launcher::validate(spec).map_err(|e| e.to_string())?;

            let mut ultimo = None;
            for (i, paso) in pasos.iter().enumerate() {
                // El numero de paso en el mensaje es la diferencia entre "la
                // macro fallo" y saber que arreglar.
                let aviso = ejecutar_spec(app, state, &paso.spec)
                    .map_err(|e| format!("Paso {} de la macro: {e}", i + 1))?;
                ultimo = aviso.or(ultimo);

                if paso.pausa_ms > 0 {
                    // Esto corre en el hilo del comando, no en el de la interfaz,
                    // asi que el panel sigue respondiendo mientras espera.
                    std::thread::sleep(std::time::Duration::from_millis(paso.pausa_ms as u64));
                }
            }
            Ok(ultimo)
        }

        otro => launcher::execute(otro).map(|_| None),
    }
}

/// Hace la captura, la guarda y la deja en el portapapeles si asi se pidio.
///
/// Devuelve la ruta del archivo, que es lo unico que el usuario necesita saber.
fn hacer_captura(
    app: &AppHandle,
    state: &State<AppState>,
    objetivo: captura::Objetivo,
) -> Result<String, String> {
    let (carpeta_ajuste, al_portapapeles) = {
        let deck = state.deck.lock().unwrap();
        (
            deck.settings.screenshot_dir.clone(),
            deck.settings.screenshot_to_clipboard,
        )
    };

    // El panel se aparta de la foto: capturar "toda la pantalla" y que salga el
    // boton que acabas de pulsar no es lo que nadie espera. Con la ventana activa
    // no hace falta, porque PrintWindow la dibuja ella misma.
    let pantalla_entera = objetivo == captura::Objetivo::Pantalla;
    let panel = pantalla_entera
        .then(|| app.get_webview_window("main"))
        .flatten();
    if let Some(w) = &panel {
        let _ = w.hide();
        // La burbuja esta siempre encima de todo: si no se aparta tambien, sale
        // en todas y cada una de las capturas de pantalla completa.
        burbuja::visible(app, false);
        // Sin esta pausa el compositor no ha terminado de repintar y el panel
        // sale igualmente en la captura.
        std::thread::sleep(std::time::Duration::from_millis(140));
    }

    let imagen = captura::capturar(objetivo);

    if let Some(w) = &panel {
        let _ = w.show();
        let nivel = state.deck.lock().unwrap().settings.window_level;
        nivel::aplicar(w, nivel);
        burbuja::visible(app, true);
    }

    let imagen = imagen?;
    let carpeta = captura::carpeta_elegida(&carpeta_ajuste, captura::carpeta_por_defecto());
    let destino = captura::guardar(&imagen, &carpeta)?;

    // El portapapeles va despues de guardar y no aborta nada: el archivo ya esta
    // en disco, y perder la captura entera porque el portapapeles estaba ocupado
    // seria un mal negocio.
    if al_portapapeles {
        if let Err(e) = al_portapapeles_imagen(app, &imagen) {
            eprintln!("[MiDeck] no se pudo copiar la captura al portapapeles: {e}");
        }
    }

    Ok(destino.display().to_string())
}

/// Deja la imagen en el portapapeles, lista para pegar.
fn al_portapapeles_imagen(app: &AppHandle, img: &image::RgbaImage) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;

    let imagen = tauri::image::Image::new(img.as_raw(), img.width(), img.height());
    app.clipboard()
        .write_image(&imagen)
        .map_err(|e| e.to_string())
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
    app: AppHandle,
    x: i32,
    y: i32,
    window: tauri::Window,
    state: State<AppState>,
) -> Result<(), String> {
    let pos = WindowPos { x, y };
    if !screen::es_alcanzable(pos, tam_ventana(&window), &monitores(&window)) {
        return Ok(());
    }
    let esquina = {
        let mut deck = state.deck.lock().unwrap();
        deck.settings.window = Some(pos);
        store::save(&deck, &state.config_path).map_err(|e| e.to_string())?;
        deck.settings.bubble.then_some(deck.settings.bubble_corner)
    };

    // Si el panel se ha ido a otra pantalla, la burbuja va detras: dejarla en el
    // monitor de antes es la misma queja de "la enciendo y no la veo", solo que
    // ahora por haber movido el panel. No crea nada, solo la recoloca, asi que
    // vale un comando sincrono.
    if let Some(esquina) = esquina {
        let _ = burbuja::colocar(&app, esquina);
    }
    Ok(())
}

/// Cambia donde vive el panel: normal, encima de todo, o al nivel del escritorio.
#[tauri::command]
fn set_window_level(
    level: WindowLevel,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        nivel::aplicar(&w, level);
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.pin.set_checked(level == WindowLevel::Top);
    }
    let mut deck = state.deck.lock().unwrap();
    deck.settings.window_level = level;
    store::save(&deck, &state.config_path).map_err(|e| e.to_string())
}

/// Candado de posicion: con el puesto, la barra de titulo no arrastra la ventana.
#[tauri::command]
fn set_lock_position(value: bool, state: State<AppState>) -> Result<(), String> {
    let mut deck = state.deck.lock().unwrap();
    deck.settings.lock_position = value;
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
        confirm_required: botones_a_confirmar(&deck),
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

// ------------------------------------------------------------------ perfiles

/// Perfil tal y como lo necesita la interfaz de Ajustes.
#[derive(Serialize)]
pub struct PerfilInfo {
    id: String,
    surface: String,
    /// Nombre de la superficie, que es lo que el usuario reconoce.
    nombre: String,
    exes: Vec<String>,
    enabled: bool,
    /// Cuantas teclas tiene configuradas, para no borrar trabajo sin saberlo.
    teclas: usize,
}

#[tauri::command]
fn list_profiles(state: State<AppState>) -> Vec<PerfilInfo> {
    let deck = state.deck.lock().unwrap();
    deck.profiles
        .iter()
        .map(|p| {
            let s = deck.surfaces.get(&p.surface);
            PerfilInfo {
                id: p.id.clone(),
                surface: p.surface.clone(),
                nombre: s.map(|s| s.name.clone()).unwrap_or_default(),
                exes: p.exes.clone(),
                enabled: p.enabled,
                teclas: s
                    .map(|s| s.pages.iter().map(|pg| pg.buttons.len()).sum())
                    .unwrap_or(0),
            }
        })
        .collect()
}

#[tauri::command]
fn create_profile(exe: String, name: String, state: State<AppState>) -> Result<DeckView, String> {
    mutar(&state, |d| edit::create_profile(d, &exe, &name).map(|_| ()))
}

#[tauri::command]
fn delete_profile(profile_id: String, state: State<AppState>) -> Result<DeckView, String> {
    mutar(&state, |d| edit::delete_profile(d, &profile_id))
}

#[tauri::command]
fn set_profile_enabled(
    profile_id: String,
    value: bool,
    state: State<AppState>,
) -> Result<DeckView, String> {
    mutar(&state, |d| edit::set_profile_enabled(d, &profile_id, value))
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
async fn update_settings(
    app: AppHandle,
    settings: model::Settings,
    state: State<'_, AppState>,
) -> Result<DeckView, String> {
    let encendida = settings.bubble;
    let esquina = settings.bubble_corner;

    let vista = mutar(&state, |d| {
        d.settings = settings;
        Ok(())
    })?;

    // Asincrono a proposito: construir una ventana de forma sincrona dentro de un
    // comando bloquea el hilo principal y la ventana sale en blanco. Ya costo una
    // tarde con el editor; aqui se evita de salida.
    burbuja::aplicar(&app, encendida, esquina)?;
    Ok(vista)
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

/// Icono que le corresponde a un destino, para la vista previa del editor.
///
/// El panel ya pinta el icono real de cada aplicacion, porque `IconSource::Auto`
/// lo extrae del destino al cargar el deck. El editor no podia: dibujaba un
/// generico hasta que guardabas e ibas a mirar como habia quedado. Con esto
/// ensena el mismo icono que se vera, mientras eliges.
///
/// Devuelve `None` sin ruido cuando no hay icono que sacar: un destino a medio
/// escribir no es un error, es lo normal mientras se teclea.
#[tauri::command]
fn icon_for_target(target: String) -> Option<String> {
    let expandido = launcher::expand_env(&target);
    if expandido.trim().is_empty() {
        return None;
    }
    let ruta = launcher::resolve_program(&expandido)?;
    icons::shell_icon(&ruta)
        .ok()
        .map(|p| p.display().to_string())
}

/// Aplicaciones instaladas, para elegirlas por nombre en el editor.
///
/// Se calcula al abrir el editor y no se cachea: instalar algo y no verlo en la
/// lista hasta reiniciar el widget seria peor que recorrer dos carpetas.
#[tauri::command]
fn list_installed_apps() -> Vec<apps::AppInstalada> {
    apps::listar()
}

/// Lo que se le manda al panel cuando cambia la aplicacion en primer plano.
///
/// Quien decide el perfil es Rust y no el frontend: la funcion de emparejamiento
/// es pura y esta cubierta por tests, y los perfiles viven en el deck. El
/// ejecutable viaja igualmente, porque es lo util para diagnosticar por que un
/// perfil no se activa.
#[derive(Clone, Serialize)]
struct PerfilActivo {
    exe: String,
    /// Superficie a mostrar. `None` = ningun perfil cubre esta aplicacion.
    surface: Option<String>,
    /// Su nombre, para ensenarlo en el panel.
    nombre: Option<String>,
}

/// Perfil que corresponde a un ejecutable, con la superficie ya comprobada.
///
/// Un perfil que apunta a una superficie borrada no se informa: mandar el panel a
/// una superficie que no existe solo produciria una rejilla en blanco.
fn perfil_activo_de(deck: &Deck, exe: &str) -> (Option<String>, Option<String>) {
    match perfiles::perfil_para(&deck.profiles, exe) {
        Some(p) => match deck.surfaces.get(&p.surface) {
            Some(s) => (Some(p.surface.clone()), Some(s.name.clone())),
            None => (None, None),
        },
        None => (None, None),
    }
}

/// Aplicaciones con ventana visible ahora mismo.
///
/// Es de aqui de donde se elige la aplicacion de un perfil: el nombre que se
/// guarda es exactamente el que el gancho de primer plano vera despues, asi que
/// no hay forma de que no empareje.
#[tauri::command]
fn list_running_apps() -> Vec<focus::AppEnEjecucion> {
    focus::apps_en_ejecucion()
}

/// Catalogo de acciones de Windows, agrupado por familia.
///
/// Lo pide el editor para construir su desplegable y el panel para saber que
/// icono le toca a cada comando. Sale de Rust y no de una copia en JavaScript
/// para que no haya dos listas que puedan separarse.
#[tauri::command]
fn list_system_commands() -> Vec<sistema::Grupo> {
    sistema::agrupado()
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

/// La burbuja pulsada: el mismo conmutador que el icono de la bandeja.
///
/// Comparten comando en vez de tener dos copias que acaben separandose.
#[tauri::command]
fn burbuja_pulsada(app: AppHandle) -> Result<(), String> {
    let Some(panel) = app.get_webview_window("main") else {
        return Err("No hay panel que mostrar.".to_string());
    };
    alternar_ventana(&panel);
    Ok(())
}

fn alternar_ventana(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        nivel::marcar_al_frente(false);
        let _ = window.hide();
    } else {
        // Mismo motivo que al relanzar: mostrar sin marcarlo acaba con el panel
        // al fondo del orden z y la sensacion de que no paso nada.
        nivel::al_frente(window);
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

fn construir_bandeja(app: &AppHandle, nivel_actual: WindowLevel) -> tauri::Result<()> {
    let autostart_activo = app.autolaunch().is_enabled().unwrap_or(false);

    let mostrar = MenuItem::with_id(app, "mostrar", "Mostrar u ocultar", true, None::<&str>)?;
    let pin = CheckMenuItem::with_id(
        app,
        "pin",
        "Siempre encima",
        true,
        nivel_actual == WindowLevel::Top,
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
                    let marcado = items.pin.is_checked().unwrap_or(false);
                    // Desde la bandeja solo se alterna entre encima y normal. El
                    // nivel escritorio se elige en Ajustes, donde cabe explicarlo.
                    let nuevo = if marcado {
                        WindowLevel::Top
                    } else {
                        WindowLevel::Normal
                    };
                    if let Some(w) = ventana {
                        nivel::aplicar(&w, nuevo);
                    }
                    let state = app.state::<AppState>();
                    let mut deck = state.deck.lock().unwrap();
                    deck.settings.window_level = nuevo;
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

/// Registra el atajo global que trae el panel al frente.
///
/// Se ignora en silencio si la combinacion ya la tiene otra aplicacion: es un
/// extra, no una razon para que el widget no arranque.
fn registrar_atajo(app: &AppHandle, combinacion: Option<&str>) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let Some(combinacion) = combinacion.filter(|c| !c.trim().is_empty()) else {
        return;
    };
    let atajos = app.global_shortcut();
    let _ = atajos.unregister_all();

    let manejador = app.clone();
    let r = atajos.on_shortcut(combinacion, move |_app, _sc, evento| {
        if evento.state() != ShortcutState::Pressed {
            return;
        }
        if let Some(w) = manejador.get_webview_window("main") {
            let visible = w.is_visible().unwrap_or(false);
            let enfocada = w.is_focused().unwrap_or(false);
            if !(visible && enfocada) {
                nivel::al_frente(&w);
                return;
            }
            // Segunda pulsacion con el panel ya delante: quitarlo de en medio.
            // En nivel escritorio se devuelve al fondo en vez de ocultarlo, porque
            // ahi su gracia es seguir estando en su sitio cuando miras el
            // escritorio; ocultarlo seria romper justo eso.
            let state = manejador.state::<AppState>();
            let nivel_actual = state.deck.lock().unwrap().settings.window_level;
            nivel::marcar_al_frente(false);
            if nivel_actual == WindowLevel::Desktop {
                nivel::al_fondo(&w);
            } else {
                let _ = w.hide();
            }
        }
    });
    if r.is_err() {
        eprintln!("[MiDeck] no se pudo registrar el atajo {combinacion}");
    }
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

    let nivel_inicial = cargado.deck.settings.window_level;
    let atajo = cargado.deck.settings.hotkey.clone();
    let start_minimized = cargado.deck.settings.start_minimized;
    let burbuja_on = cargado.deck.settings.bubble;
    let burbuja_esquina = cargado.deck.settings.bubble_corner;
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
            // Volver a lanzar el ejecutable es la forma en que un usuario pide ver
            // el panel. Tiene que venir al frente: con un show() a secas, en nivel
            // escritorio el manejador de foco lo devolvia al fondo y parecia que
            // la aplicacion no respondia.
            if let Some(w) = app.get_webview_window("main") {
                nivel::al_frente(&w);
            }
        }));
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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
            set_window_level,
            set_lock_position,
            set_autostart,
            open_config_file,
            burbuja_pulsada,
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
            list_system_commands,
            list_installed_apps,
            list_running_apps,
            list_profiles,
            create_profile,
            delete_profile,
            set_profile_enabled,
            icon_for_target,
            notify_deck_changed
        ])
        .setup(move |app| {
            // El protocolo asset: solo sirve lo que se le permite explicitamente.
            // Sin esto las teclas saldrian sin imagen y sin ningun error visible.
            let scope = app.asset_protocol_scope();
            let _ = scope.allow_directory(images::library_dir(), true);
            let _ = scope.allow_directory(icons::cache_dir(), true);

            construir_bandeja(app.handle(), nivel_inicial)?;

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
            nivel::aplicar(&window, nivel_inicial);
            // Sin esto, un atajo dirigido a otra aplicacion llegaria al panel.
            // El gancho avisa cada vez que la aplicacion en primer plano cambia de
            // verdad. Quien decide que hacer con eso es esta capa, no focus.rs.
            {
                let mango = app.handle().clone();
                focus::vigilar_primer_plano(move |ruta| {
                    let exe = focus::nombre_de_ejecutable(&ruta);
                    let (surface, nombre) = {
                        let estado = mango.state::<AppState>();
                        let deck = estado.deck.lock().unwrap();
                        perfil_activo_de(&deck, &exe)
                    };
                    // Solo en compilacion de desarrollo: en produccion seria una linea
                    // por cada Alt+Tab del dia.
                    #[cfg(debug_assertions)]
                    eprintln!("[MiDeck] primer plano: {exe} -> perfil {nombre:?}");
                    let aviso = PerfilActivo {
                        exe,
                        surface,
                        nombre,
                    };

                    // El salto al hilo principal no es ceremonia: un `emit` desde
                    // un hilo propio devuelve Ok y **no entrega nada**. Se ve
                    // enseguida porque los comandos sincronos de Tauri ya corren
                    // en el principal, asi que el mismo evento emitido desde un
                    // comando si llegaba. Solo salta el aviso; buscar el
                    // ejecutable y emparejar el perfil se queda en el trabajador.
                    let emisor = mango.clone();
                    let _ = mango.run_on_main_thread(move || {
                        let _ = emisor.emit("perfil-activo", aviso);
                    });
                });
            }
            registrar_atajo(app.handle(), atajo.as_deref());

            if let Err(e) = burbuja::aplicar(app.handle(), burbuja_on, burbuja_esquina) {
                eprintln!("[MiDeck] no se pudo crear la burbuja: {e}");
            }

            if !start_minimized {
                let _ = window.show();
                let _ = window.set_focus();
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // En nivel escritorio hay que reimponer el fondo del orden z: Windows
            // no tiene una bandera "siempre debajo" que lo mantenga solo.
            if let WindowEvent::Focused(enfocada) = event {
                if window.label() == "main" {
                    let state = window.state::<AppState>();
                    let nivel_actual = state.deck.lock().unwrap().settings.window_level;
                    let reaccion =
                        nivel::al_cambiar_foco(nivel_actual, *enfocada, nivel::esta_al_frente());
                    if reaccion == nivel::Reaccion::Empujar {
                        if !*enfocada {
                            nivel::marcar_al_frente(false);
                        }
                        if let Some(w) = window.get_webview_window("main") {
                            nivel::al_fondo(&w);
                        }
                    }
                }
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                // Solo el panel se oculta a la bandeja: es un widget, no una
                // ventana de documento. El editor se cierra de verdad, o quedaria
                // invisible ocupando memoria y la proxima apertura lo recrearia.
                if window.label() == "main" {
                    api.prevent_close();
                    guardar_posicion(window);

                    // En nivel escritorio, ocultar seria quitarle al panel su
                    // unica razon de ser: estar siempre en su sitio. Se manda al
                    // fondo, que es donde vive, y asi la X nunca lo hace
                    // "desaparecer" a ojos de quien lo usa.
                    let state = window.state::<AppState>();
                    let nivel_actual = state.deck.lock().unwrap().settings.window_level;
                    nivel::marcar_al_frente(false);
                    if nivel_actual == WindowLevel::Desktop {
                        if let Some(w) = window.get_webview_window("main") {
                            nivel::al_fondo(&w);
                        }
                    } else {
                        let _ = window.hide();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error al arrancar MiDeck");
}
