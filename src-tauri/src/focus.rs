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
