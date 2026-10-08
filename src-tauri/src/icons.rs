//! Extraccion del icono de shell de un archivo (`"icon": {"type": "auto"}`).
//!
//! Es el unico punto donde Tauri cuesta mas que PyQt6, donde seria una linea.
//! El camino elegido:
//!
//!   SHGetFileInfoW(SHGFI_SYSICONINDEX)  ->  indice en la lista de imagenes del sistema
//!   SHGetImageList(SHIL_JUMBO)          ->  HICON de 256 px
//!   GetIconInfo + GetDIBits             ->  pixeles BGRA
//!
//! Se usa SHGetFileInfoW sin SHGFI_USEFILEATTRIBUTES a proposito: asi un .lnk
//! devuelve el icono real de su destino en vez del generico de acceso directo.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Carpeta de cache. Va en LOCALAPPDATA, no en APPDATA: es contenido regenerable
/// que no tiene por que viajar con el perfil movil del usuario.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base)
        .join("MiDeck")
        .join("cache")
        .join("icons")
}

/// Clave de cache: ruta + fecha de modificacion. Si la app se actualiza y cambia
/// su icono, la clave cambia sola y se vuelve a extraer.
fn clave(target: &Path) -> String {
    let mtime = target
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut hasher = Sha256::new();
    hasher.update(target.to_string_lossy().to_lowercase().as_bytes());
    hasher.update(mtime.to_le_bytes());
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Devuelve la ruta a un PNG con el icono del archivo, extrayendolo si hace falta.
pub fn shell_icon(target: &Path) -> Result<PathBuf, String> {
    if !target.exists() {
        return Err(format!("No existe: {}", target.display()));
    }

    let destino = cache_dir().join(format!("{}.png", clave(target)));
    if destino.exists() {
        return Ok(destino);
    }

    let img = extraer(target)?;
    std::fs::create_dir_all(cache_dir()).map_err(|e| e.to_string())?;
    img.save(&destino).map_err(|e| e.to_string())?;
    Ok(destino)
}

#[cfg(not(windows))]
fn extraer(_target: &Path) -> Result<image::RgbaImage, String> {
    Err("La extraccion de iconos solo esta implementada en Windows.".into())
}

#[cfg(windows)]
fn extraer(target: &Path) -> Result<image::RgbaImage, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE,
        SHIL_JUMBO, SHIL_LARGE,
    };
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

    use std::os::windows::ffi::OsStrExt;
    let ancho: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();

    unsafe {
        let mut info = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(ancho.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        );
        if ok == 0 {
            return Err(format!(
                "Windows no reporto icono para {}",
                target.display()
            ));
        }

        // De mayor a menor: si el jumbo no tiene arte real para este tipo, se baja
        // un escalon en vez de devolver una tecla vacia.
        for nivel in [SHIL_JUMBO, SHIL_EXTRALARGE, SHIL_LARGE] {
            let lista: IImageList = match SHGetImageList(nivel as i32) {
                Ok(l) => l,
                Err(_) => continue,
            };
            let Ok(hicon) = lista.GetIcon(info.iIcon, ILD_TRANSPARENT.0) else {
                continue;
            };

            let resultado = hicon_a_rgba(hicon);
            let _ = DestroyIcon(hicon);

            if let Ok(img) = resultado {
                if let Some(recortada) = recortar_transparencia(img) {
                    return Ok(recortada);
                }
            }
        }
        Err(format!(
            "No se pudo convertir el icono de {}",
            target.display()
        ))
    }
}

/// Convierte un HICON en pixeles RGBA.
#[cfg(windows)]
unsafe fn hicon_a_rgba(
    hicon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> Result<image::RgbaImage, String> {
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut ii = ICONINFO::default();
    GetIconInfo(hicon, &mut ii).map_err(|e| e.to_string())?;

    let mut bmp = BITMAP::default();
    GetObjectW(
        ii.hbmColor,
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bmp as *mut _ as *mut _),
    );

    let (w, h) = (bmp.bmWidth.max(0) as u32, bmp.bmHeight.max(0) as u32);
    if w == 0 || h == 0 {
        let _ = DeleteObject(ii.hbmColor);
        let _ = DeleteObject(ii.hbmMask);
        return Err("El icono no tiene dimensiones".into());
    }

    let mut cabecera = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            // Negativo = filas de arriba hacia abajo, que es el orden que espera
            // image. Con el valor positivo la tecla saldria del reves.
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let hdc: HDC = GetDC(None);
    let mut color = vec![0u8; (w * h * 4) as usize];
    let leidas = GetDIBits(
        hdc,
        ii.hbmColor,
        0,
        h,
        Some(color.as_mut_ptr() as *mut _),
        &mut cabecera,
        DIB_RGB_COLORS,
    );

    // Los iconos antiguos de 32 bits traen el canal alfa en cero: la transparencia
    // real vive en la mascara. Sin esto, saldrian teclas completamente invisibles.
    let sin_alfa = color.chunks_exact(4).all(|p| p[3] == 0);
    let mut mascara = vec![0u8; (w * h * 4) as usize];
    if sin_alfa {
        GetDIBits(
            hdc,
            ii.hbmMask,
            0,
            h,
            Some(mascara.as_mut_ptr() as *mut _),
            &mut cabecera,
            DIB_RGB_COLORS,
        );
    }

    ReleaseDC(None, hdc);
    let _ = DeleteObject(ii.hbmColor);
    let _ = DeleteObject(ii.hbmMask);

    if leidas == 0 {
        return Err("GetDIBits no devolvio pixeles".into());
    }

    // GDI entrega BGRA; image espera RGBA.
    for (i, p) in color.chunks_exact_mut(4).enumerate() {
        p.swap(0, 2);
        if sin_alfa {
            // En la mascara, negro = opaco.
            let m = mascara[i * 4];
            p[3] = if m == 0 { 255 } else { 0 };
        }
    }

    image::RgbaImage::from_raw(w, h, color).ok_or_else(|| "Buffer de tamano inesperado".into())
}

/// Recorta el marco transparente.
///
/// Un icono pequeno dentro de un lienzo jumbo de 256 px llega centrado con mucho
/// relleno vacio; sin recortar, en la tecla se veria diminuto. Devuelve None si la
/// imagen esta enteramente vacia, que es la senal para bajar de nivel.
fn recortar_transparencia(img: image::RgbaImage) -> Option<image::RgbaImage> {
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);

    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 > x1 || y0 > y1 {
        return None;
    }

    let recorte = image::imageops::crop_imm(&img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
    Some(recorte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_clave_de_cache_depende_de_la_ruta() {
        let a = std::env::temp_dir().join("mideck-clave-a.txt");
        let b = std::env::temp_dir().join("mideck-clave-b.txt");
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "x").unwrap();
        assert_ne!(clave(&a), clave(&b));
        assert_eq!(clave(&a), clave(&a), "la clave debe ser estable");
    }

    #[test]
    fn un_destino_inexistente_no_intenta_extraer() {
        let r = shell_icon(Path::new("C:\\no\\existe\\jamas.exe"));
        assert!(r.is_err());
    }

    #[test]
    fn el_recorte_elimina_el_marco_vacio() {
        // Lienzo de 256 con solo 4 px opacos en el centro: debe quedar en 4x4.
        let mut img = image::RgbaImage::new(256, 256);
        for y in 120..124 {
            for x in 100..104 {
                img.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let r = recortar_transparencia(img).expect("hay pixeles opacos");
        assert_eq!(r.dimensions(), (4, 4));
    }

    #[test]
    fn una_imagen_vacia_se_rechaza_para_poder_bajar_de_nivel() {
        let img = image::RgbaImage::new(256, 256);
        assert!(recortar_transparencia(img).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn extrae_el_icono_real_de_un_ejecutable_del_sistema() {
        let notepad = Path::new("C:\\Windows\\System32\\notepad.exe");
        if !notepad.exists() {
            return;
        }
        let img = extraer(notepad).expect("notepad.exe debe tener icono");
        let (w, h) = img.dimensions();
        // Con SHIL_JUMBO el arte util ronda los 256 px; se exige bastante mas que
        // los 32 px que daria ExtractIconExW, que es justo por lo que no se uso.
        assert!(w >= 48 && h >= 48, "icono demasiado pequeno: {w}x{h}");
        assert!(
            img.pixels().any(|p| p.0[3] > 0),
            "el icono salio completamente transparente"
        );
    }
}
