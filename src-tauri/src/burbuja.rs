//! La burbuja flotante que devuelve el panel.
//!
//! Con el panel al nivel del escritorio, traerlo exige acordarse del atajo global
//! o encontrar el icono en la bandeja. Es la misma raiz del reporte de «se cierra
//! y no puedo volver a abrirla»: el panel no se cerraba, se escondia donde nadie
//! sabia buscarlo.
//!
//! Lo delicado no es dibujarla, es **no robar el foco**. Si al pulsarla Windows se
//! lo quita a la aplicacion que el usuario tiene delante, se rompe todo lo de la
//! fase 5: `devolver_foco()` deja de tener a donde volver y los atajos llegan al
//! sitio equivocado.

use crate::model::BubbleCorner;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Etiqueta de la ventana. Tiene que estar declarada en `capabilities`, o la
/// pagina carga y toda llamada suya falla en ejecucion.
pub const ETIQUETA: &str = "burbuja";

/// Lado en puntos. Suficiente para acertarle sin mirar y poco para estorbar.
pub const LADO: i32 = 56;

/// Separacion respecto al borde de la pantalla.
pub const MARGEN: i32 = 16;

/// Donde va la esquina superior izquierda de la burbuja dentro de un area util.
///
/// Pura: el area de trabajo entra como parametro para poder probar las cuatro
/// esquinas sin pantalla. Se calcula contra el **area de trabajo** y no contra la
/// pantalla entera, que es lo que evita que acabe debajo de la barra de tareas
/// este donde este.
pub fn posicion(
    esquina: BubbleCorner,
    area: (i32, i32, i32, i32),
    lado: i32,
    margen: i32,
) -> (i32, i32) {
    let (izq, arr, der, aba) = area;
    match esquina {
        BubbleCorner::BottomRight => (der - lado - margen, aba - lado - margen),
        BubbleCorner::BottomLeft => (izq + margen, aba - lado - margen),
        BubbleCorner::TopRight => (der - lado - margen, arr + margen),
        BubbleCorner::TopLeft => (izq + margen, arr + margen),
    }
}

/// El area de trabajo del escritorio: la pantalla menos la barra de tareas.
#[cfg(windows)]
pub fn area_de_trabajo() -> (i32, i32, i32, i32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    let mut r = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut r as *mut RECT as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_err() {
        // Sin area de trabajo no se inventa nada raro: la esquina de arriba a la
        // izquierda siempre es visible.
        return (0, 0, 800, 600);
    }
    (r.left, r.top, r.right, r.bottom)
}

#[cfg(not(windows))]
pub fn area_de_trabajo() -> (i32, i32, i32, i32) {
    (0, 0, 800, 600)
}

/// Pone la burbuja como digan los ajustes: la crea, la mueve o la quita.
///
/// Un solo sitio decide, y lo llaman tanto el arranque como el guardado de
/// Ajustes: asi no hay forma de que el ajuste diga una cosa y la pantalla otra.
pub fn aplicar(app: &AppHandle, encendida: bool, esquina: BubbleCorner) -> Result<(), String> {
    if !encendida {
        if let Some(v) = app.get_webview_window(ETIQUETA) {
            // Destruir y no esconder: un WebView2 escondido sigue costando
            // memoria, que es justo lo que se quiere evitar al apagarla.
            let _ = v.destroy();
        }
        return Ok(());
    }

    if app.get_webview_window(ETIQUETA).is_some() {
        return colocar(app, esquina);
    }
    crear(app, esquina)
}

/// Mueve la burbuja a una esquina sin recrearla.
pub fn colocar(app: &AppHandle, esquina: BubbleCorner) -> Result<(), String> {
    let Some(ventana) = app.get_webview_window(ETIQUETA) else {
        return Ok(());
    };
    let (x, y) = posicion(esquina, area_de_trabajo(), LADO, MARGEN);

    #[cfg(windows)]
    if let Ok(mango) = ventana.hwnd() {
        ajustar(windows::Win32::Foundation::HWND(mango.0), x, y, LADO);
    }
    #[cfg(not(windows))]
    let _ = (ventana, x, y);

    Ok(())
}

/// Esconde o devuelve la burbuja. Para las capturas de pantalla completa: si no,
/// saldria en todas.
pub fn visible(app: &AppHandle, mostrar: bool) {
    let Some(ventana) = app.get_webview_window(ETIQUETA) else {
        return;
    };
    let _ = if mostrar {
        ventana.show()
    } else {
        ventana.hide()
    };
}

/// Crea la ventana de la burbuja si no existe.
pub fn crear(app: &AppHandle, esquina: BubbleCorner) -> Result<(), String> {
    if app.get_webview_window(ETIQUETA).is_some() {
        return Ok(());
    }

    let (x, y) = posicion(esquina, area_de_trabajo(), LADO, MARGEN);

    let ventana = WebviewWindowBuilder::new(app, ETIQUETA, WebviewUrl::App("burbuja.html".into()))
        .title("MiDeck")
        .inner_size(LADO as f64, LADO as f64)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()
        .map_err(|e| e.to_string())?;

    #[cfg(windows)]
    if let Ok(mango) = ventana.hwnd() {
        // Tauri trae su propia version del crate `windows`, asi que el handle no
        // es el mismo tipo aunque sea el mismo numero: se reconstruye.
        ajustar(windows::Win32::Foundation::HWND(mango.0), x, y, LADO);
    }
    #[cfg(not(windows))]
    let _ = (ventana, x, y);

    Ok(())
}

/// Deja la ventana como tiene que ser: sin marco, del tamano pedido y sin robar
/// el foco.
///
/// Son tres cosas que Tauri no expone y que hay que hacer de una pasada, porque
/// cambiar los estilos obliga a reaplicar la geometria.
///
/// - `WS_EX_NOACTIVATE` es lo que hace que Windows no la active al pulsarla, y
///   `WS_EX_TOOLWINDOW` la saca del Alt+Tab, donde una burbuja de 56 px no pinta
///   nada.
/// - `WS_POPUP` a secas, sin menu de sistema ni marco de redimensionado: medido
///   en este equipo, una ventana que los conserva **no baja de 136 px de ancho**
///   por mucho que se le pida 56, porque Windows le reserva sitio a unos botones
///   que aqui no existen.
/// - `SetWindowPos` trabaja siempre en pixeles fisicos, que es justo lo que da
///   `SPI_GETWORKAREA`: asi la burbuja cae donde debe sin pasar por la conversion
///   a unidades logicas, que con la pantalla escalada la mandaba fuera.
#[cfg(windows)]
fn ajustar(hwnd: windows::Win32::Foundation::HWND, x: i32, y: i32, lado: i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
    };

    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(
            hwnd,
            GWL_EXSTYLE,
            ex | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize),
        );
        SetWindowLongPtrW(hwnd, GWL_STYLE, ((WS_POPUP | WS_VISIBLE).0) as isize);

        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            lado,
            lado,
            SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }

    recortar_en_circulo(hwnd, lado);
}

/// Deja pasar el raton por las esquinas de fuera del circulo.
///
/// La ventana es cuadrada y el boton es redondo: sin esto, los cuatro trozos
/// transparentes de las esquinas se comerian los clics de lo que haya debajo, y
/// el usuario veria desaparecer pulsaciones sin entender por que.
#[cfg(windows)]
fn recortar_en_circulo(hwnd: windows::Win32::Foundation::HWND, lado: i32) {
    use windows::Win32::Graphics::Gdi::{CreateEllipticRgn, DeleteObject, SetWindowRgn};

    unsafe {
        let region = CreateEllipticRgn(0, 0, lado + 1, lado + 1);
        if region.is_invalid() {
            return;
        }
        // Windows se queda con la region: no hay que destruirla aqui. Si
        // SetWindowRgn fallara, se libera para no dejarla colgando.
        if SetWindowRgn(hwnd, region, true) == 0 {
            let _ = DeleteObject(region);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Un area de trabajo de 1920x1080 con la barra de tareas abajo, 40 px.
    const AREA: (i32, i32, i32, i32) = (0, 0, 1920, 1152);

    #[test]
    fn cada_esquina_cae_dentro_del_area_de_trabajo() {
        for esquina in [
            BubbleCorner::BottomRight,
            BubbleCorner::BottomLeft,
            BubbleCorner::TopRight,
            BubbleCorner::TopLeft,
        ] {
            let (x, y) = posicion(esquina, AREA, LADO, MARGEN);
            assert!(x >= AREA.0, "{esquina:?} se sale por la izquierda");
            assert!(y >= AREA.1, "{esquina:?} se sale por arriba");
            assert!(x + LADO <= AREA.2, "{esquina:?} se sale por la derecha");
            assert!(
                y + LADO <= AREA.3,
                "{esquina:?} quedaria debajo de la barra de tareas"
            );
        }
    }

    #[test]
    fn la_barra_de_tareas_a_un_lado_tambien_la_desplaza() {
        // Barra vertical a la izquierda: el area util no empieza en cero, y la
        // burbuja de la izquierda tiene que respetarlo.
        let con_barra_lateral = (72, 0, 1920, 1152);
        let (x, _) = posicion(BubbleCorner::BottomLeft, con_barra_lateral, LADO, MARGEN);
        assert_eq!(x, 72 + MARGEN);
    }

    #[test]
    fn un_monitor_secundario_a_la_izquierda_da_coordenadas_negativas() {
        // El area de trabajo del escritorio puede empezar en negativo, y eso es
        // una posicion valida: recortarla a cero la mandaria a otra pantalla.
        let area = (-1440, 0, 0, 2512);
        let (x, y) = posicion(BubbleCorner::BottomLeft, area, LADO, MARGEN);
        assert_eq!((x, y), (-1424, 2440));
    }
}
