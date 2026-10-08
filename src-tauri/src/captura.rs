//! Captura de pantalla propia.
//!
//! La captura de region ya la resuelve Windows con `Win+Shift+S`, porque exige
//! una superposicion de seleccion que no merece la pena reimplementar. Lo que
//! Windows no da en una sola tecla es "toda la pantalla" o "esta ventana" sin
//! pasar por una interfaz, y eso es lo que hace este modulo.
//!
//! El camino es el mismo GDI que ya usa `icons.rs`, con una diferencia que cuesta
//! una tarde descubrir: **BitBlt no escribe el canal alfa**. Los bytes que deja
//! ahi son basura, casi siempre cero, asi que el PNG sale completamente
//! transparente y la captura parece vacia. Hay que forzarlo a opaco.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Objetivo {
    /// Todo el escritorio virtual, con los monitores que haya conectados.
    Pantalla,
    /// La ventana que estaba delante **antes** de pulsar el panel.
    VentanaActiva,
}

// ------------------------------------------------------------- donde se guarda

/// Carpeta por defecto: `Imagenes\MiDeck` del usuario.
pub fn carpeta_por_defecto() -> PathBuf {
    let base = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join("Pictures").join("MiDeck")
}

/// Carpeta efectiva: la configurada si tiene algo, si no la de por defecto.
///
/// Pura para poder probarla: elegir mal la carpeta significa capturas que el
/// usuario no encuentra, y eso no se nota hasta que ya pasó.
pub fn carpeta_elegida(ajuste: &str, por_defecto: PathBuf) -> PathBuf {
    let limpio = crate::launcher::expand_env(ajuste);
    if limpio.trim().is_empty() {
        por_defecto
    } else {
        PathBuf::from(limpio.trim())
    }
}

/// `captura-AAAAMMDD-HHMMSS.png`, en hora local.
///
/// En local y no en UTC a proposito: el nombre lo lee una persona que acaba de
/// pulsar la tecla y espera ver la hora de su reloj, no cinco horas por delante.
pub fn nombre_de_archivo(y: u16, m: u16, d: u16, hh: u16, mm: u16, ss: u16) -> String {
    format!("captura-{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}.png")
}

/// Si el nombre ya existe, se le anade un sufijo en vez de pisar la captura
/// anterior. Dos pulsaciones en el mismo segundo son raras, pero perder una
/// captura por eso seria absurdo.
fn sin_colision(carpeta: &Path, nombre: &str) -> PathBuf {
    let candidato = carpeta.join(nombre);
    if !candidato.exists() {
        return candidato;
    }
    let raiz = nombre.trim_end_matches(".png");
    for n in 2..100 {
        let otro = carpeta.join(format!("{raiz}-{n}.png"));
        if !otro.exists() {
            return otro;
        }
    }
    candidato
}

/// Guarda la imagen y devuelve la ruta final.
pub fn guardar(img: &image::RgbaImage, carpeta: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(carpeta)
        .map_err(|e| format!("No se pudo crear {}: {e}", carpeta.display()))?;

    let (y, m, d, hh, mm, ss) = ahora_local();
    let destino = sin_colision(carpeta, &nombre_de_archivo(y, m, d, hh, mm, ss));
    img.save(&destino)
        .map_err(|e| format!("No se pudo guardar {}: {e}", destino.display()))?;
    Ok(destino)
}

#[cfg(windows)]
fn ahora_local() -> (u16, u16, u16, u16, u16, u16) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

#[cfg(not(windows))]
fn ahora_local() -> (u16, u16, u16, u16, u16, u16) {
    (1970, 1, 1, 0, 0, 0)
}

// -------------------------------------------------------------- la captura

#[cfg(not(windows))]
pub fn capturar(_objetivo: Objetivo) -> Result<image::RgbaImage, String> {
    Err("La captura de pantalla solo esta implementada en Windows.".into())
}

#[cfg(windows)]
pub fn capturar(objetivo: Objetivo) -> Result<image::RgbaImage, String> {
    match objetivo {
        Objetivo::Pantalla => pantalla_completa(),
        Objetivo::VentanaActiva => ventana_activa(),
    }
}

/// Todo el escritorio virtual, no solo el monitor principal.
#[cfg(windows)]
fn pantalla_completa() -> Result<image::RgbaImage, String> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        ReleaseDC, SelectObject, CAPTUREBLT, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let ancho = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let alto = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if ancho <= 0 || alto <= 0 {
            return Err("Windows no reporto el tamano del escritorio.".into());
        }

        let pantalla = GetDC(None);
        let memoria = CreateCompatibleDC(pantalla);
        let mapa = CreateCompatibleBitmap(pantalla, ancho, alto);
        let anterior = SelectObject(memoria, mapa);

        // CAPTUREBLT incluye las ventanas con transparencia por capas, que si no
        // salen como agujeros negros.
        let copiado = BitBlt(
            memoria,
            0,
            0,
            ancho,
            alto,
            pantalla,
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        );

        let resultado = if copiado.is_ok() {
            mapa_a_rgba(pantalla, mapa, ancho as u32, alto as u32)
        } else {
            Err("BitBlt no pudo copiar la pantalla.".to_string())
        };

        SelectObject(memoria, anterior);
        let _ = DeleteObject(mapa);
        let _ = DeleteDC(memoria);
        ReleaseDC(None, pantalla);
        resultado
    }
}

#[cfg(windows)]
fn ventana_activa() -> Result<image::RgbaImage, String> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        ReleaseDC, SelectObject, CAPTUREBLT, SRCCOPY,
    };
    // PrintWindow vive en Storage::Xps, no en WindowsAndMessaging, porque la API
    // nacio para imprimir. Nada que ver con el uso que se le da aqui.
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    // La ventana que interesa es la que el usuario tenia delante, no el panel:
    // al pulsar la tecla el panel ya es la ventana en primer plano.
    let hwnd = crate::focus::ventana_anterior()
        .ok_or_else(|| "No hay ninguna ventana delante que capturar.".to_string())?;

    unsafe {
        let mut marco = RECT::default();
        GetWindowRect(hwnd, &mut marco).map_err(|e| e.to_string())?;
        let ancho = marco.right - marco.left;
        let alto = marco.bottom - marco.top;
        if ancho <= 0 || alto <= 0 {
            return Err("La ventana no tiene tamano visible.".into());
        }

        let pantalla = GetDC(None);
        let memoria = CreateCompatibleDC(pantalla);
        let mapa = CreateCompatibleBitmap(pantalla, ancho, alto);
        let anterior = SelectObject(memoria, mapa);

        // PW_RENDERFULLCONTENT pide a la ventana que se dibuje ella misma, asi
        // que sale entera aunque el panel la estuviera tapando. Si la aplicacion
        // no lo soporta se cae a copiar lo que haya en pantalla.
        const PW_RENDERFULLCONTENT: u32 = 0x0000_0002;
        let pintada =
            PrintWindow(hwnd, memoria, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
        if !pintada {
            let _ = BitBlt(
                memoria,
                0,
                0,
                ancho,
                alto,
                pantalla,
                marco.left,
                marco.top,
                SRCCOPY | CAPTUREBLT,
            );
        }

        let resultado = mapa_a_rgba(pantalla, mapa, ancho as u32, alto as u32);

        SelectObject(memoria, anterior);
        let _ = DeleteObject(mapa);
        let _ = DeleteDC(memoria);
        ReleaseDC(None, pantalla);

        let img = resultado?;
        Ok(recortar_marco_invisible(hwnd, marco, img))
    }
}

/// Quita el borde invisible de redimension que Windows deja alrededor.
///
/// `GetWindowRect` devuelve un rectangulo mayor que lo que se ve: sobran unos
/// pixeles transparentes a los lados y abajo. DWM sabe cual es el marco real, y
/// la diferencia entre ambos es lo que hay que recortar para que la captura no
/// lleve un cerco negro.
#[cfg(windows)]
fn recortar_marco_invisible(
    hwnd: windows::Win32::Foundation::HWND,
    marco: windows::Win32::Foundation::RECT,
    img: image::RgbaImage,
) -> image::RgbaImage {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};

    let mut real = RECT::default();
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut real as *mut _ as *mut std::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if ok.is_err() {
        return img;
    }

    let Some((x, y, w, h)) = recorte(
        (marco.left, marco.top, marco.right, marco.bottom),
        (real.left, real.top, real.right, real.bottom),
        img.width(),
        img.height(),
    ) else {
        return img;
    };

    image::imageops::crop_imm(&img, x, y, w, h).to_image()
}

/// Rectangulo a recortar, en coordenadas de la imagen. `None` si no hay nada que
/// recortar o si las cuentas no cuadran, en cuyo caso vale mas dejar la captura
/// entera que devolver una franja.
pub fn recorte(
    marco: (i32, i32, i32, i32),
    real: (i32, i32, i32, i32),
    ancho: u32,
    alto: u32,
) -> Option<(u32, u32, u32, u32)> {
    let x = (real.0 - marco.0).max(0) as u32;
    let y = (real.1 - marco.1).max(0) as u32;
    let w = (real.2 - real.0).max(0) as u32;
    let h = (real.3 - real.1).max(0) as u32;

    if w == 0 || h == 0 || x + w > ancho || y + h > alto {
        return None;
    }
    if x == 0 && y == 0 && w == ancho && h == alto {
        return None; // nada que recortar
    }
    Some((x, y, w, h))
}

/// Pasa un mapa de bits de GDI a RGBA.
#[cfg(windows)]
fn mapa_a_rgba(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    mapa: windows::Win32::Graphics::Gdi::HBITMAP,
    ancho: u32,
    alto: u32,
) -> Result<image::RgbaImage, String> {
    use windows::Win32::Graphics::Gdi::{
        GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };

    let mut cabecera = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ancho as i32,
            // Negativo = filas de arriba hacia abajo. Con el positivo la captura
            // sale del reves.
            biHeight: -(alto as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bytes = vec![0u8; (ancho as usize) * (alto as usize) * 4];
    let leidas = unsafe {
        GetDIBits(
            hdc,
            mapa,
            0,
            alto,
            Some(bytes.as_mut_ptr() as *mut _),
            &mut cabecera,
            DIB_RGB_COLORS,
        )
    };
    if leidas == 0 {
        return Err("No se pudieron leer los pixeles de la captura.".into());
    }

    bgra_a_rgba_opaco(&mut bytes);
    image::RgbaImage::from_raw(ancho, alto, bytes)
        .ok_or_else(|| "La captura no tiene el tamano esperado.".to_string())
}

/// GDI entrega BGRA, e `image` espera RGBA. Y el alfa se fuerza a opaco: BitBlt
/// no lo escribe, asi que sin esto el PNG sale entero transparente y la captura
/// parece haber salido en blanco.
pub fn bgra_a_rgba_opaco(bytes: &mut [u8]) {
    for p in bytes.chunks_exact_mut(4) {
        p.swap(0, 2);
        p[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_nombre_lleva_fecha_y_hora_con_ceros() {
        assert_eq!(
            nombre_de_archivo(2026, 10, 8, 9, 5, 3),
            "captura-20261008-090503.png"
        );
    }

    #[test]
    fn sin_carpeta_configurada_se_usa_la_de_por_defecto() {
        let d = PathBuf::from("C:\\Por\\Defecto");
        assert_eq!(carpeta_elegida("", d.clone()), d);
        assert_eq!(carpeta_elegida("   ", d.clone()), d);
    }

    #[test]
    fn la_carpeta_configurada_manda_y_expande_variables() {
        std::env::set_var("MIDECK_CAPTURAS", "C:\\Mis\\Capturas");
        assert_eq!(
            carpeta_elegida("%MIDECK_CAPTURAS%", PathBuf::from("C:\\Otra")),
            PathBuf::from("C:\\Mis\\Capturas")
        );
    }

    #[test]
    fn bgra_se_convierte_a_rgba_y_queda_opaco() {
        // Un pixel azul puro en BGRA, con el alfa a cero como lo deja BitBlt.
        let mut bytes = vec![255, 0, 0, 0];
        bgra_a_rgba_opaco(&mut bytes);
        assert_eq!(bytes, vec![0, 0, 255, 255]);
    }

    #[test]
    fn el_alfa_a_cero_de_bitblt_no_deja_la_captura_invisible() {
        // El fallo concreto que esto evita: todo transparente.
        let mut bytes = vec![10, 20, 30, 0, 40, 50, 60, 0];
        bgra_a_rgba_opaco(&mut bytes);
        assert!(bytes.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn se_recorta_el_borde_invisible_de_redimension() {
        // Caso real de Windows 11: siete pixeles sobrantes a cada lado y abajo.
        let marco = (93, 93, 1107, 707);
        let real = (100, 100, 1100, 700);
        assert_eq!(recorte(marco, real, 1014, 614), Some((7, 7, 1000, 600)));
    }

    #[test]
    fn sin_borde_sobrante_no_se_recorta() {
        let r = (0, 0, 800, 600);
        assert_eq!(recorte(r, r, 800, 600), None);
    }

    #[test]
    fn un_recorte_que_se_sale_de_la_imagen_se_descarta() {
        // Mas vale la captura entera que una franja: si DWM reporta algo que no
        // cuadra con lo capturado, no se toca.
        let marco = (0, 0, 800, 600);
        let real = (0, 0, 900, 700);
        assert_eq!(recorte(marco, real, 800, 600), None);
    }
}
