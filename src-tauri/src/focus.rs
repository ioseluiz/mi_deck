//! Traer al frente una aplicacion que ya esta abierta.
//!
//! Es lo que espera cualquiera de una tecla de Stream Deck: pulsar "Outlook" diez
//! veces no deberia dejar diez Outlooks, sino volver al que ya estaba.
//!
//! Se recorren las ventanas de nivel superior buscando una cuyo proceso tenga el
//! mismo ejecutable que el destino. Se descartan las ventanas con propietario
//! (dialogos, tooltips) y las que no tienen titulo, que no son la ventana
//! principal de nada.

use std::path::Path;

/// Intenta enfocar una ventana de ese ejecutable. Devuelve true si lo consiguio.
#[cfg(not(windows))]
pub fn focus_running(_exe: &Path) -> bool {
    false
}

#[cfg(windows)]
pub fn focus_running(exe: &Path) -> bool {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let objetivo = normalizar(exe);
    if objetivo.is_empty() {
        return false;
    }

    struct Busqueda {
        objetivo: String,
        encontrada: HWND,
    }

    let mut busqueda = Busqueda {
        objetivo,
        encontrada: HWND::default(),
    };

    unsafe extern "system" fn visitar(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::Foundation::TRUE;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindow, GetWindowTextLengthW, IsWindowVisible, GW_OWNER,
        };

        let busqueda = &mut *(lparam.0 as *mut Busqueda);

        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        // Una ventana con propietario es un dialogo o similar, no la principal.
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return TRUE;
        }
        if GetWindowTextLengthW(hwnd) == 0 {
            return TRUE;
        }

        if let Some(ruta) = ejecutable_de_ventana(hwnd) {
            if ruta == busqueda.objetivo {
                busqueda.encontrada = hwnd;
                return windows::Win32::Foundation::FALSE;
            }
        }
        TRUE
    }

    unsafe {
        let _ = EnumWindows(
            Some(visitar),
            LPARAM(&mut busqueda as *mut Busqueda as isize),
        );

        if busqueda.encontrada.is_invalid() {
            return false;
        }
        let hwnd = busqueda.encontrada;
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd).as_bool()
    }
}

// ------------------------------------------- recordar quien tenia el foco antes

use std::sync::atomic::{AtomicIsize, Ordering};

/// Ultima ventana en primer plano que no era nuestra.
///
/// Al pulsar una tecla del panel, MiDeck toma el foco; un atajo dirigido a otra
/// aplicacion (Ctrl+S en Excel) llegaria aqui y no alla. Hay que recordar a donde
/// devolverlo.
///
/// Se guarda como entero y no como HWND porque un HWND no es atomico; 0 significa
/// "no hay ninguna recordada".
static VENTANA_ANTERIOR: AtomicIsize = AtomicIsize::new(0);

/// Cuanto se espera a que la racha de cambios se calme antes de informar.
///
/// Un Alt+Tab, abrir un menu o cerrar un dialogo disparan el evento varias veces
/// seguidas. Sin esta pausa, el panel parpadearia cambiando de perfil tres veces
/// para acabar donde iba a acabar de todas formas.
#[cfg(windows)]
const AMORTIGUACION: std::time::Duration = std::time::Duration::from_millis(250);

/// Por donde el gancho avisa al hilo trabajador. Acotado y con `try_send`: el
/// gancho corre en el hilo de la interfaz y no puede bloquearse nunca.
#[cfg(windows)]
static EMISOR: std::sync::OnceLock<std::sync::mpsc::SyncSender<isize>> = std::sync::OnceLock::new();

/// Instala el gancho que vigila los cambios de ventana en primer plano.
///
/// Se descarto leer `GetForegroundWindow()` desde `WindowEvent::Focused(true)`:
/// cuando ese evento llega, MiDeck YA es la ventana en primer plano y la lectura
/// devuelve la nuestra. El gancho es el unico sitio donde se ve la transicion.
///
/// `WINEVENT_SKIPOWNPROCESS` hace que no se dispare para nuestras propias
/// ventanas, asi que no hay que filtrar por identificador de proceso. Eso vale
/// tambien para la ventana del editor, que es del mismo proceso: abrirla no
/// contara como cambio de aplicacion.
///
/// `avisar` recibe la ruta del ejecutable cada vez que la aplicacion en primer
/// plano **cambia de verdad**. Se pasa como cierre para que este modulo siga sin
/// saber nada de Tauri.
#[cfg(windows)]
pub fn vigilar_primer_plano(avisar: impl Fn(String) + Send + 'static) {
    use windows::Win32::Foundation::{HMODULE, HWND};
    use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    // El gancho corre en el hilo que bombea mensajes, que es el de la interfaz.
    // Por eso aqui solo se guarda el identificador y se manda por el canal: abrir
    // el proceso y leer su ruta es trabajo que no tiene por que hacer el hilo que
    // pinta.
    unsafe extern "system" fn al_cambiar(
        _gancho: HWINEVENTHOOK,
        _evento: u32,
        hwnd: HWND,
        _objeto: i32,
        _hijo: i32,
        _hilo: u32,
        _tiempo: u32,
    ) {
        if hwnd.is_invalid() {
            return;
        }
        let id = hwnd.0 as isize;
        VENTANA_ANTERIOR.store(id, Ordering::Relaxed);
        if let Some(emisor) = EMISOR.get() {
            // Si la cola esta llena da igual perder este: solo importa el ultimo.
            let _ = emisor.try_send(id);
        }
    }

    arrancar_trabajador(avisar);

    unsafe {
        let gancho = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            HMODULE::default(),
            Some(al_cambiar),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if gancho.is_invalid() {
            eprintln!(
                "[MiDeck] no se pudo vigilar el primer plano: los atajos dirigidos a                  otra aplicacion llegaran al panel en vez de a ella."
            );
        }
        // El gancho se deja instalado el resto de la vida del proceso: no hay
        // momento en que convenga dejar de saber a donde devolver el foco.
    }
}

/// Hilo que amortigua la racha de cambios, resuelve el ejecutable y avisa.
#[cfg(windows)]
fn arrancar_trabajador(avisar: impl Fn(String) + Send + 'static) {
    use std::sync::mpsc::{sync_channel, RecvTimeoutError};
    use windows::Win32::Foundation::HWND;

    let (emisor, receptor) = sync_channel::<isize>(8);
    if EMISOR.set(emisor).is_err() {
        return; // ya habia uno: no instalar dos trabajadores
    }

    std::thread::spawn(move || {
        let mut ultimo: Option<String> = None;

        // Siembra inicial: si MiDeck arranca mientras tienes Excel delante, el
        // perfil de Excel tiene que estar puesto ya. Sin esto habria que salir de
        // la aplicacion y volver para que el gancho se enterara.
        if let Some(ruta) = ejecutable_actual() {
            ultimo = Some(ruta.clone());
            avisar(ruta);
        }

        while let Ok(primero) = receptor.recv() {
            // Quedarse con el ultimo de la racha.
            let mut id = primero;
            loop {
                match receptor.recv_timeout(AMORTIGUACION) {
                    Ok(siguiente) => id = siguiente,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }

            let hwnd = HWND(id as *mut core::ffi::c_void);
            let Some(ruta) = ejecutable_de(hwnd) else {
                continue;
            };
            // Cambiar entre dos ventanas de la misma aplicacion no es un cambio
            // de aplicacion, y repintar el panel por eso solo distrae.
            if ultimo.as_deref() == Some(ruta.as_str()) {
                continue;
            }
            ultimo = Some(ruta.clone());
            avisar(ruta);
        }
    });
}

#[cfg(not(windows))]
pub fn vigilar_primer_plano(_avisar: impl Fn(String) + Send + 'static) {}

/// Identificador de la ventana recordada, si sigue existiendo.
#[cfg(windows)]
pub fn ventana_anterior() -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;

    let guardada = VENTANA_ANTERIOR.load(Ordering::Relaxed);
    if guardada == 0 {
        return None;
    }
    let hwnd = HWND(guardada as *mut core::ffi::c_void);
    // La ventana recordada puede haberse cerrado desde entonces.
    if unsafe { IsWindow(hwnd) }.as_bool() {
        Some(hwnd)
    } else {
        VENTANA_ANTERIOR.store(0, Ordering::Relaxed);
        None
    }
}

/// Devuelve el foco a la aplicacion que lo tenia antes del panel.
///
/// Devuelve false si no habia ninguna recordada o si Windows rechazo el cambio.
/// Quien llama decide si sigue adelante: para un atajo de sistema (Win+algo) da
/// igual quien tenga el foco, pero para Ctrl+S en Excel es imprescindible.
#[cfg(windows)]
pub fn devolver_foco() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let Some(hwnd) = ventana_anterior() else {
        return false;
    };
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            return false;
        }
    }
    // Windows necesita un instante para completar el cambio de foco antes de que
    // la entrada sintetica llegue a la ventana correcta.
    std::thread::sleep(std::time::Duration::from_millis(60));
    true
}

#[cfg(not(windows))]
pub fn devolver_foco() -> bool {
    false
}

// ------------------------------------------------- aplicaciones abiertas ahora

/// Una aplicacion con ventana visible en este momento.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AppEnEjecucion {
    /// Nombre del ejecutable, que es la clave de los perfiles: "excel.exe".
    pub exe: String,
    /// Titulo de una de sus ventanas, para reconocerla en la lista.
    pub titulo: String,
}

/// Quita los repetidos por ejecutable, descarta el propio MiDeck y ordena.
///
/// Pura para poder probarla: una lista con Chrome ocho veces no ayuda a nadie a
/// elegir, y el orden estable evita que la lista baile entre dos aperturas. Lo de
/// descartarse a si mismo no es cosmetico: el gancho nunca se dispara por nuestras
/// ventanas, asi que un perfil para MiDeck no se activaria jamas y solo serviria
/// para que alguien perdiera un rato averiguando por que.
pub fn ordenar_apps(mut v: Vec<AppEnEjecucion>, propio: &str) -> Vec<AppEnEjecucion> {
    v.retain(|a| a.exe != propio);
    v.sort_by(|a, b| a.exe.cmp(&b.exe).then_with(|| a.titulo.cmp(&b.titulo)));
    v.dedup_by(|a, b| a.exe == b.exe);
    v
}

/// Nombre del ejecutable de MiDeck, para no ofrecerse a si mismo.
fn exe_propio() -> String {
    std::env::current_exe()
        .map(|p| nombre_de_ejecutable(&p.to_string_lossy()))
        .unwrap_or_default()
}

/// Aplicaciones con ventana visible ahora mismo.
///
/// Se eligen de aqui y no del Menu Inicio a proposito: `apps::listar()` devuelve
/// accesos directos (.lnk) y el gancho devuelve ejecutables (.exe), asi que lo que
/// se guardara nunca emparejaria con lo que se detecta. Tomandolo de una ventana
/// abierta, el nombre es exactamente el que el gancho vera despues.
#[cfg(not(windows))]
pub fn apps_en_ejecucion() -> Vec<AppEnEjecucion> {
    Vec::new()
}

#[cfg(windows)]
pub fn apps_en_ejecucion() -> Vec<AppEnEjecucion> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    let mut encontradas: Vec<AppEnEjecucion> = Vec::new();

    unsafe extern "system" fn visitar(hwnd: HWND, lparam: LPARAM) -> BOOL {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindow, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible, GW_OWNER,
        };

        let salida = &mut *(lparam.0 as *mut Vec<AppEnEjecucion>);

        // Los mismos tres filtros que ya usa focus_running: invisible, con
        // propietario (un dialogo) o sin titulo no es la ventana principal de nada.
        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return TRUE;
        }
        let largo = GetWindowTextLengthW(hwnd);
        if largo == 0 {
            return TRUE;
        }

        let Some(ruta) = ejecutable_de_ventana(hwnd) else {
            return TRUE;
        };

        let mut titulo = vec![0u16; largo as usize + 1];
        let escritos = GetWindowTextW(hwnd, &mut titulo);

        salida.push(AppEnEjecucion {
            exe: nombre_de_ejecutable(&ruta),
            titulo: String::from_utf16_lossy(&titulo[..escritos.max(0) as usize]),
        });
        TRUE
    }

    unsafe {
        let _ = EnumWindows(
            Some(visitar),
            LPARAM(&mut encontradas as *mut Vec<AppEnEjecucion> as isize),
        );
    }
    ordenar_apps(encontradas, &exe_propio())
}

/// Ruta del ejecutable del proceso dueno de una ventana.
#[cfg(windows)]
unsafe fn ejecutable_de_ventana(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return None;
    }

    // PROCESS_QUERY_LIMITED_INFORMATION basta y funciona con procesos de otra
    // sesion o mas privilegiados, donde QUERY_INFORMATION fallaria.
    let proceso = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

    let mut buffer = [0u16; 32_768];
    let mut largo = buffer.len() as u32;
    let ok = QueryFullProcessImageNameW(
        proceso,
        PROCESS_NAME_FORMAT(0),
        PWSTR(buffer.as_mut_ptr()),
        &mut largo,
    );
    let _ = CloseHandle(proceso);

    if ok.is_err() || largo == 0 {
        return None;
    }
    let ruta = String::from_utf16_lossy(&buffer[..largo as usize]);
    Some(normalizar(Path::new(&ruta)))
}

/// Ejecutable de la ventana que esta en primer plano ahora mismo.
///
/// Solo para la siembra inicial. Devuelve `None` si la ventana de delante es
/// nuestra, que al arrancar es lo normal: un perfil para MiDeck no existe.
#[cfg(windows)]
fn ejecutable_actual() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let ruta = ejecutable_de(hwnd)?;
    (nombre_de_ejecutable(&ruta) != exe_propio()).then_some(ruta)
}

/// Ruta del ejecutable de una ventana, ya normalizada.
///
/// Envoltura segura de la version interna, para que el resto del programa no
/// tenga que escribir `unsafe` solo por preguntar de quien es una ventana.
#[cfg(windows)]
pub fn ejecutable_de(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    unsafe { ejecutable_de_ventana(hwnd) }
}

/// Clave de comparacion entre rutas de ejecutable.
///
/// Windows no distingue mayusculas en rutas, y el mismo programa puede llegar
/// escrito de formas distintas desde deck.json y desde la API.
fn normalizar(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().replace('/', "\\")
}

/// Nombre del ejecutable, sin ruta y en minusculas: `excel.exe`.
///
/// Es la clave con la que se emparejaran los perfiles por aplicacion. Se usa el
/// nombre y no la ruta completa a proposito: el mismo programa vive en sitios
/// distintos segun se instale por usuario o por maquina --Office y Chrome son los
/// casos tipicos--, y un perfil con la ruta de un equipo no serviria en el de al
/// lado.
pub fn nombre_de_ejecutable(ruta: &str) -> String {
    ruta.rsplit(['\\', '/'])
        .next()
        .unwrap_or(ruta)
        .trim()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_comparacion_de_rutas_ignora_mayusculas_y_barras() {
        assert_eq!(
            normalizar(Path::new("C:/Windows/System32/NOTEPAD.EXE")),
            normalizar(Path::new("c:\\windows\\system32\\notepad.exe"))
        );
    }

    #[test]
    fn un_ejecutable_que_nadie_esta_corriendo_no_enfoca_nada() {
        assert!(!focus_running(Path::new(
            "C:\\no\\existe\\programa_inventado.exe"
        )));
    }

    // ------------------------------------------ la clave de los perfiles

    #[test]
    fn el_nombre_del_ejecutable_se_queda_sin_ruta_y_en_minusculas() {
        assert_eq!(
            nombre_de_ejecutable(r"C:\Program Files\Microsoft Office\root\Office16\EXCEL.EXE"),
            "excel.exe"
        );
        assert_eq!(
            nombre_de_ejecutable("C:/Windows/notepad.exe"),
            "notepad.exe"
        );
    }

    /// El motivo de comparar por nombre y no por ruta: la misma aplicacion
    /// instalada por usuario y por maquina tiene que dar la misma clave.
    #[test]
    fn la_misma_app_en_dos_rutas_da_la_misma_clave() {
        assert_eq!(
            nombre_de_ejecutable(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            nombre_de_ejecutable(r"C:\Users\alguien\AppData\Local\Google\Chrome\chrome.exe")
        );
    }

    #[test]
    fn un_nombre_suelto_se_queda_como_esta() {
        assert_eq!(nombre_de_ejecutable("Excel.exe"), "excel.exe");
        assert_eq!(nombre_de_ejecutable(""), "");
    }

    #[test]
    fn la_lista_de_apps_no_repite_ejecutables() {
        // Diez ventanas de Chrome son una sola entrada en la lista.
        let v = ordenar_apps(
            vec![
                AppEnEjecucion {
                    exe: "chrome.exe".into(),
                    titulo: "Pestana 2".into(),
                },
                AppEnEjecucion {
                    exe: "excel.exe".into(),
                    titulo: "Libro1".into(),
                },
                AppEnEjecucion {
                    exe: "chrome.exe".into(),
                    titulo: "Pestana 1".into(),
                },
            ],
            "mideck.exe",
        );
        let exes: Vec<&str> = v.iter().map(|a| a.exe.as_str()).collect();
        assert_eq!(exes, vec!["chrome.exe", "excel.exe"]);
        // Y se queda con el primero por titulo, para que la lista no baile.
        assert_eq!(v[0].titulo, "Pestana 1");
    }
}
