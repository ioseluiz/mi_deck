//! Nivel de la ventana: normal, siempre encima, o al nivel del escritorio.
//!
//! Windows no tiene una bandera "siempre debajo" equivalente a WS_EX_TOPMOST.
//! El nivel escritorio se consigue empujando la ventana al fondo del orden z con
//! `SetWindowPos(HWND_BOTTOM)` y volviendo a hacerlo cada vez que algo la saca de
//! ahi, que en la practica es cada vez que recibe el foco.
//!
//! Hay una excepcion deliberada: cuando el atajo global la trae al frente, no se
//! la empuja abajo hasta que vuelve a perder el foco. Si no, el atajo la mostraria
//! y la escondería en el mismo instante.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::model::WindowLevel;

/// Cierto mientras el atajo global mantiene el panel al frente a proposito.
static AL_FRENTE: AtomicBool = AtomicBool::new(false);

pub fn marcar_al_frente(valor: bool) {
    AL_FRENTE.store(valor, Ordering::Relaxed);
}

pub fn esta_al_frente() -> bool {
    AL_FRENTE.load(Ordering::Relaxed)
}

/// Aplica el nivel a la ventana.
pub fn aplicar(window: &tauri::WebviewWindow, nivel: WindowLevel) {
    match nivel {
        WindowLevel::Top => {
            let _ = window.set_always_on_top(true);
        }
        WindowLevel::Normal => {
            let _ = window.set_always_on_top(false);
        }
        WindowLevel::Desktop => {
            let _ = window.set_always_on_top(false);
            marcar_al_frente(false);
            al_fondo(window);
        }
    }
}

/// Empuja la ventana al fondo del orden z, por debajo de todas las demas.
#[cfg(windows)]
pub fn al_fondo(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    let Ok(handle) = window.hwnd() else {
        return;
    };
    unsafe {
        let _ = SetWindowPos(
            HWND(handle.0),
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            // Sin mover ni redimensionar, y sin robar el foco al hacerlo.
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
pub fn al_fondo(_window: &tauri::WebviewWindow) {}

/// Trae la ventana al frente, pase lo que pase con su nivel.
///
/// Lo usa el atajo global: aunque el panel viva al nivel del escritorio, pulsarlo
/// tiene que ponerlo delante de todo o el atajo no serviria de nada.
pub fn al_frente(window: &tauri::WebviewWindow) {
    marcar_al_frente(true);
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();

    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, SetWindowPos, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE,
        };
        if let Ok(handle) = window.hwnd() {
            unsafe {
                let h = HWND(handle.0);
                let _ = SetWindowPos(h, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                let _ = SetForegroundWindow(h);
            }
        }
    }
}

/// Que hacer cuando la ventana gana o pierde el foco, segun su nivel.
///
/// Separado de Tauri para poder probar la decision sin arrancar una ventana.
#[derive(Debug, PartialEq, Eq)]
pub enum Reaccion {
    /// Dejarla donde esta.
    Nada,
    /// Empujarla al fondo del orden z.
    Empujar,
}

pub fn al_cambiar_foco(nivel: WindowLevel, enfocada: bool, traida_por_atajo: bool) -> Reaccion {
    if nivel != WindowLevel::Desktop {
        return Reaccion::Nada;
    }
    if enfocada {
        // Si la trajo el atajo, se respeta hasta que el usuario se vaya a otra
        // ventana. Empujarla aqui la haria desaparecer nada mas aparecer.
        if traida_por_atajo {
            Reaccion::Nada
        } else {
            Reaccion::Empujar
        }
    } else {
        // Al perder el foco vuelve a su sitio, y se cancela el permiso del atajo.
        Reaccion::Empujar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_nivel_normal_o_encima_no_se_toca_nada() {
        for nivel in [WindowLevel::Normal, WindowLevel::Top] {
            assert_eq!(al_cambiar_foco(nivel, true, false), Reaccion::Nada);
            assert_eq!(al_cambiar_foco(nivel, false, false), Reaccion::Nada);
        }
    }

    #[test]
    fn en_nivel_escritorio_un_clic_la_devuelve_al_fondo() {
        assert_eq!(
            al_cambiar_foco(WindowLevel::Desktop, true, false),
            Reaccion::Empujar
        );
    }

    #[test]
    fn el_atajo_la_mantiene_al_frente_mientras_tenga_el_foco() {
        // Sin esto, el atajo la mostraria y la escondería en el mismo instante.
        assert_eq!(
            al_cambiar_foco(WindowLevel::Desktop, true, true),
            Reaccion::Nada
        );
    }

    #[test]
    fn al_irse_a_otra_ventana_vuelve_al_fondo_aunque_la_trajera_el_atajo() {
        assert_eq!(
            al_cambiar_foco(WindowLevel::Desktop, false, true),
            Reaccion::Empujar
        );
    }
}
