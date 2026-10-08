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

/// Instala el gancho que vigila los cambios de ventana en primer plano.
///
/// Se descarto leer `GetForegroundWindow()` desde `WindowEvent::Focused(true)`:
/// cuando ese evento llega, MiDeck YA es la ventana en primer plano y la lectura
/// devuelve la nuestra. El gancho es el unico sitio donde se ve la transicion.
///
/// `WINEVENT_SKIPOWNPROCESS` hace que no se dispare para nuestras propias
/// ventanas, asi que no hay que filtrar por identificador de proceso.
#[cfg(windows)]
pub fn vigilar_primer_plano() {
    use windows::Win32::Foundation::{HMODULE, HWND};
    use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    unsafe extern "system" fn al_cambiar(
        _gancho: HWINEVENTHOOK,
        _evento: u32,
        hwnd: HWND,
        _objeto: i32,
        _hijo: i32,
        _hilo: u32,
        _tiempo: u32,
    ) {
        if !hwnd.is_invalid() {
            VENTANA_ANTERIOR.store(hwnd.0 as isize, Ordering::Relaxed);
        }
    }

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

#[cfg(not(windows))]
pub fn vigilar_primer_plano() {}

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

/// Clave de comparacion entre rutas de ejecutable.
///
/// Windows no distingue mayusculas en rutas, y el mismo programa puede llegar
/// escrito de formas distintas desde deck.json y desde la API.
fn normalizar(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().replace('/', "\\")
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
}
