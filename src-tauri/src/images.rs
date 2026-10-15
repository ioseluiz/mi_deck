//! Biblioteca de imagenes del usuario.
//!
//! Las imagenes no se referencian por ruta absoluta: se importan a
//! %APPDATA%\MiDeck\icons. Si deck.json apuntara a C:\...\Descargas\logo.png, el
//! boton se romperia en cuanto se vaciara Descargas, y el deck no se podria copiar
//! a otro equipo. Con biblioteca, deck.json + la carpeta icons son un paquete
//! autocontenido.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use sha2::{Digest, Sha256};

use crate::model::{Deck, IconSource};

/// Lado mayor al que se normaliza. 256 px mantiene la tecla nitida si se sube
/// key_size o en pantallas HiDPI, y cuesta unos 40 KB por imagen.
const LADO: u32 = 256;

/// Tope para guardar un GIF animado tal cual. Por encima se guarda el primer
/// fotograma, que es lo que MiDeck hacia con todos hasta ahora.
///
/// Un GIF animado no se puede reescalar sin recodificar la animacion entera, asi
/// que entra con el tamano y las dimensiones que traiga: decodificarlo lo paga el
/// webview en cada vuelta, para siempre, aunque la tecla mida 72 px.
const MAX_GIF: usize = 2 * 1024 * 1024;

/// Formatos que se aceptan al importar.
const EXTENSIONES: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "ico", "gif", "tiff", "tif", "svg",
];

#[derive(Debug)]
pub enum ImageError {
    NoExiste(PathBuf),
    FormatoNoSoportado(String),
    NoSePudoLeer(String),
    NoSePudoEscribir(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::NoExiste(p) => write!(f, "No se encontro la imagen: {}", p.display()),
            ImageError::FormatoNoSoportado(e) => write!(
                f,
                "El formato .{e} no se admite. Usa PNG, JPG, WEBP, BMP, ICO, GIF o SVG."
            ),
            ImageError::NoSePudoLeer(e) => write!(f, "No se pudo leer la imagen: {e}"),
            ImageError::NoSePudoEscribir(e) => write!(f, "No se pudo guardar la imagen: {e}"),
        }
    }
}

impl std::error::Error for ImageError {}

// ------------------------------------------------------------------ ubicacion

/// Carpeta de la biblioteca, junto a deck.json.
pub fn library_dir() -> PathBuf {
    crate::store::config_path()
        .parent()
        .map(|p| p.join("icons"))
        .unwrap_or_else(|| PathBuf::from("icons"))
}

/// Originales sin tocar, por si luego hace falta reprocesarlos con otro recorte
/// sin volver a pedirle el archivo al usuario.
pub fn originals_dir() -> PathBuf {
    library_dir().join("original")
}

/// Ruta absoluta de una imagen de la biblioteca a partir del nombre guardado en
/// deck.json.
pub fn resolve(file: &str) -> PathBuf {
    library_dir().join(file)
}

// ------------------------------------------------------------------ importar

/// Importa una imagen a la biblioteca y devuelve el nombre de archivo que debe
/// guardarse en deck.json.
///
/// El nombre es el hash del contenido, asi que importar dos veces la misma imagen
/// no duplica el archivo y renombrar el original no rompe nada.
pub fn import(src: &Path) -> Result<String, ImageError> {
    if !src.is_file() {
        return Err(ImageError::NoExiste(src.to_path_buf()));
    }

    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !EXTENSIONES.contains(&ext.as_str()) {
        return Err(ImageError::FormatoNoSoportado(ext));
    }

    let bytes = fs::read(src).map_err(|e| ImageError::NoSePudoLeer(e.to_string()))?;
    let hash = hash_corto(&bytes);

    fs::create_dir_all(originals_dir()).map_err(|e| ImageError::NoSePudoEscribir(e.to_string()))?;

    // Hay dos formatos que se guardan tal cual, y se decide por el contenido y no
    // por la extension: quien renombra un .gif a .png no cambia lo que hay dentro.
    if let Some(ext) = intacto(&bytes) {
        let nombre = format!("{hash}.{ext}");
        let destino = library_dir().join(&nombre);
        if !destino.exists() {
            fs::write(&destino, &bytes).map_err(|e| ImageError::NoSePudoEscribir(e.to_string()))?;
        }
        return Ok(nombre);
    }

    let nombre = format!("{hash}.png");
    let destino = library_dir().join(&nombre);
    let original = originals_dir().join(format!("{hash}.{ext}"));

    if destino.exists() {
        return Ok(nombre);
    }

    let img = image::load_from_memory(&bytes)
        .map_err(|e| ImageError::NoSePudoLeer(e.to_string()))?
        .into_rgba8();

    // Se reescala manteniendo la proporcion, sin recortar ni rellenar: el ajuste
    // final (contain o cover) lo hace el CSS de la tecla, y asi cambiar `fit`
    // despues no exige reimportar la imagen.
    let normalizada = image::DynamicImage::ImageRgba8(img).resize(LADO, LADO, FilterType::Lanczos3);

    normalizada
        .save(&destino)
        .map_err(|e| ImageError::NoSePudoEscribir(e.to_string()))?;
    let _ = fs::write(&original, &bytes);

    Ok(nombre)
}

/// Importa una imagen que llega como bytes en bruto: del portapapeles, o de un
/// perfil exportado que trae sus imagenes dentro.
pub fn import_bytes(bytes: &[u8]) -> Result<String, ImageError> {
    let hash = hash_corto(bytes);

    fs::create_dir_all(library_dir()).map_err(|e| ImageError::NoSePudoEscribir(e.to_string()))?;

    // Lo mismo que al importar por ruta. Esto antes solo sabia escribir PNG, y era
    // un agujero: un perfil exportado con un icono SVG llegaba al otro equipo *sin
    // icono*, porque `load_from_memory` no entiende SVG y la imagen se descartaba
    // en silencio en `intercambio::importar`.
    if let Some(ext) = intacto(bytes) {
        let nombre = format!("{hash}.{ext}");
        let destino = library_dir().join(&nombre);
        if !destino.exists() {
            fs::write(&destino, bytes).map_err(|e| ImageError::NoSePudoEscribir(e.to_string()))?;
        }
        return Ok(nombre);
    }

    let nombre = format!("{hash}.png");
    let destino = library_dir().join(&nombre);
    if destino.exists() {
        return Ok(nombre);
    }

    let img =
        image::load_from_memory(bytes).map_err(|e| ImageError::NoSePudoLeer(e.to_string()))?;
    img.resize(LADO, LADO, FilterType::Lanczos3)
        .save(&destino)
        .map_err(|e| ImageError::NoSePudoEscribir(e.to_string()))?;

    Ok(nombre)
}

/// Con que extension guardar unos bytes que no hay que tocar, o `None` si son una
/// imagen fija que gana normalizandose a PNG como las demas.
///
/// Se mira el contenido y no la extension a proposito: lo que decide si una imagen
/// se anima es lo que lleva dentro.
fn intacto(bytes: &[u8]) -> Option<&'static str> {
    decidir(es_svg(bytes), fotogramas(bytes), bytes.len())
}

/// La decision sola, sin leer ni escribir nada, para poder probar el tope sin
/// fabricar dos megas de GIF.
fn decidir(svg: bool, fotogramas: usize, bytes: usize) -> Option<&'static str> {
    // El webview renderiza el SVG nativo y lo escala sin perdida, asi que
    // rasterizarlo solo empeoraria el resultado.
    if svg {
        return Some("svg");
    }
    // Un GIF de un solo fotograma es una imagen fija y no gana nada quedandose
    // como GIF: pasa por el camino normal y acaba en PNG de 256 px.
    if fotogramas > 1 && bytes <= MAX_GIF {
        return Some("gif");
    }
    None
}

fn es_svg(bytes: &[u8]) -> bool {
    // Basta con el principio: un SVG puede abrir con la declaracion XML, con un
    // comentario o con la etiqueta misma, pero `<svg` aparece pronto.
    let cabeza = &bytes[..bytes.len().min(1024)];
    String::from_utf8_lossy(cabeza)
        .to_ascii_lowercase()
        .contains("<svg")
}

/// Cuantos fotogramas tiene un GIF, contando como mucho dos.
///
/// Solo hace falta saber si pasa de uno, y parar ahi es lo que evita decodificar
/// una animacion larga entera nada mas para responder que si.
fn fotogramas(bytes: &[u8]) -> usize {
    if !bytes.starts_with(b"GIF87a") && !bytes.starts_with(b"GIF89a") {
        return 0;
    }
    use image::codecs::gif::GifDecoder;
    use image::AnimationDecoder;
    let Ok(decodificador) = GifDecoder::new(std::io::Cursor::new(bytes)) else {
        return 0;
    };
    decodificador
        .into_frames()
        .take(2)
        .filter(|f| f.is_ok())
        .count()
}

fn hash_corto(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

// ----------------------------------------------------------------- limpieza

#[derive(Debug, serde::Serialize)]
pub struct ImagenSinUso {
    pub file: String,
    pub bytes: u64,
}

/// Imagenes de la biblioteca que ninguna tecla referencia.
///
/// Nunca se borran solas: al eliminar un boton su imagen se queda, y limpiarlas
/// es una accion explicita del usuario desde Ajustes. Un borrado automatico
/// convertiria un error de edicion en una perdida de trabajo.
pub fn unused(deck: &Deck) -> Vec<ImagenSinUso> {
    let usadas: HashSet<&str> = deck
        .surfaces
        .values()
        .flat_map(|s| s.pages.iter())
        .flat_map(|p| p.buttons.iter())
        .filter_map(|b| match &b.icon.source {
            IconSource::Image { file } => Some(file.as_str()),
            _ => None,
        })
        .collect();

    let mut sueltas: Vec<ImagenSinUso> = fs::read_dir(library_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let nombre = e.file_name().to_string_lossy().to_string();
            if usadas.contains(nombre.as_str()) {
                return None;
            }
            Some(ImagenSinUso {
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
                file: nombre,
            })
        })
        .collect();

    sueltas.sort_by(|a, b| a.file.cmp(&b.file));
    sueltas
}

/// Borra una imagen de la biblioteca y su original. Solo debe llamarse tras
/// confirmacion explicita del usuario.
pub fn delete(file: &str) -> Result<(), ImageError> {
    // Un nombre con separadores podria escapar de la biblioteca.
    if file.contains(['/', '\\']) || file.contains("..") {
        return Err(ImageError::FormatoNoSoportado(file.to_string()));
    }
    let destino = resolve(file);
    if destino.exists() {
        fs::remove_file(&destino).map_err(|e| ImageError::NoSePudoEscribir(e.to_string()))?;
    }
    if let Some(tallo) = Path::new(file).file_stem() {
        if let Ok(entradas) = fs::read_dir(originals_dir()) {
            for e in entradas.flatten() {
                if e.path().file_stem() == Some(tallo) {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }
    Ok(())
}

/// Cara apropiada para una tecla recien creada a partir de un archivo soltado.
///
/// Si el archivo es una imagen, se importa y la tecla la muestra. Dejarla en
/// `auto` daria el icono de shell del tipo de archivo, es decir el mismo logo
/// generico de Windows para cualquier PNG: inservible para distinguir teclas,
/// que es justo para lo que estan las imagenes.
pub fn icono_para(path: &Path) -> crate::model::Icon {
    use crate::model::{Icon, IconSource};
    if !es_imagen(path) {
        return Icon::default();
    }
    match import(path) {
        Ok(file) => Icon {
            source: IconSource::Image { file },
            ..Icon::default()
        },
        // Si no se pudo importar, mejor una tecla con icono generico que ninguna.
        Err(_) => Icon::default(),
    }
}

/// Si la ruta parece una imagen, por extension. Lo usa el manejo de arrastre para
/// decidir si soltar un archivo cambia la cara de una tecla o crea un boton.
pub fn es_imagen(path: &Path) -> bool {
    path.extension()
        .map(|e| EXTENSIONES.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static CONTADOR: AtomicU32 = AtomicU32::new(0);

    /// DECK_CONFIG es global al proceso y cargo test corre en paralelo, asi que
    /// estos tests tienen que turnarse o se pisan la biblioteca entre ellos.
    fn turno() -> MutexGuard<'static, ()> {
        static CERROJO: OnceLock<Mutex<()>> = OnceLock::new();
        CERROJO
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Apunta la biblioteca a una carpeta temporal propia de cada test.
    /// Devuelve tambien el guardia: mantenlo vivo hasta el final del test.
    fn aislar(tag: &str) -> (PathBuf, MutexGuard<'static, ()>) {
        let guardia = turno();
        let n = CONTADOR.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("mideck-img-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("DECK_CONFIG", dir.join("deck.json"));
        fs::create_dir_all(library_dir()).unwrap();
        (dir, guardia)
    }

    fn escribir_png(dir: &Path, nombre: &str, w: u32, h: u32, alfa: u8) -> PathBuf {
        let mut img = RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgba([(x % 256) as u8, (y % 256) as u8, 128, alfa]);
        }
        let ruta = dir.join(nombre);
        img.save(&ruta).unwrap();
        ruta
    }

    #[test]
    fn la_misma_imagen_dos_veces_da_un_solo_archivo() {
        let (dir, _g) = aislar("dup");
        let a = escribir_png(&dir, "a.png", 64, 64, 255);
        let b = dir.join("copia-con-otro-nombre.png");
        fs::copy(&a, &b).unwrap();

        let n1 = import(&a).unwrap();
        let n2 = import(&b).unwrap();

        assert_eq!(n1, n2, "el hash de contenido debe coincidir");
        let archivos: Vec<_> = fs::read_dir(library_dir())
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_file())
            .collect();
        assert_eq!(archivos.len(), 1, "no debe duplicarse el archivo");
    }

    #[test]
    fn una_imagen_apaisada_conserva_su_proporcion_con_el_lado_mayor_en_256() {
        // Recortar al importar impediria cambiar `fit` despues sin reimportar:
        // el ajuste lo hace el CSS, aqui solo se limita el tamano.
        let (dir, _g) = aislar("apaisada");
        let src = escribir_png(&dir, "ancha.png", 800, 400, 255);
        let nombre = import(&src).unwrap();

        let img = image::open(resolve(&nombre)).unwrap();
        assert_eq!(img.width(), 256);
        assert_eq!(img.height(), 128);
    }

    #[test]
    fn un_png_con_transparencia_la_conserva() {
        let (dir, _g) = aislar("alfa");
        let src = escribir_png(&dir, "transp.png", 128, 128, 0);
        let nombre = import(&src).unwrap();

        let img = image::open(resolve(&nombre)).unwrap().into_rgba8();
        assert!(
            img.pixels().all(|p| p.0[3] == 0),
            "el canal alfa se perdio al normalizar"
        );
    }

    #[test]
    fn un_svg_se_copia_sin_rasterizar() {
        let (dir, _g) = aislar("svg");
        let src = dir.join("icono.svg");
        let contenido = br#"<svg xmlns="http://www.w3.org/2000/svg"><circle r="4"/></svg>"#;
        fs::write(&src, contenido).unwrap();

        let nombre = import(&src).unwrap();
        assert!(nombre.ends_with(".svg"));
        assert_eq!(fs::read(resolve(&nombre)).unwrap(), contenido);
    }

    /// Un GIF de `cuantos` fotogramas, alternando tono para que no se colapsen.
    fn escribir_gif(dir: &Path, nombre: &str, cuantos: usize, lado: u32) -> PathBuf {
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame, Rgba, RgbaImage};

        let ruta = dir.join(nombre);
        {
            let mut enc = GifEncoder::new(fs::File::create(&ruta).unwrap());
            for i in 0..cuantos {
                let tono = if i % 2 == 0 { 40 } else { 200 };
                let img = RgbaImage::from_pixel(lado, lado, Rgba([tono, tono, tono, 255]));
                enc.encode_frame(Frame::from_parts(
                    img,
                    0,
                    0,
                    Delay::from_numer_denom_ms(100, 1),
                ))
                .unwrap();
            }
        }
        ruta
    }

    #[test]
    fn un_gif_animado_se_guarda_tal_cual_para_que_siga_animandose() {
        // Hasta la v0.4.1 esto acababa en un PNG del primer fotograma: la tecla se
        // quedaba quieta y no habia forma de saber por que.
        let (dir, _g) = aislar("gifanimado");
        let src = escribir_gif(&dir, "late.gif", 3, 64);

        let nombre = import(&src).unwrap();
        assert!(nombre.ends_with(".gif"), "se rasterizo: {nombre}");
        assert_eq!(
            fs::read(resolve(&nombre)).unwrap(),
            fs::read(&src).unwrap(),
            "reescribirlo perderia fotogramas"
        );
    }

    #[test]
    fn un_gif_de_un_solo_fotograma_se_normaliza_como_cualquier_imagen_fija() {
        // No hay animacion que conservar, asi que gana pasando por el camino de
        // siempre: 256 px de lado mayor y PNG.
        let (dir, _g) = aislar("gifquieto");
        let src = escribir_gif(&dir, "fijo.gif", 1, 400);

        let nombre = import(&src).unwrap();
        assert!(nombre.ends_with(".png"), "{nombre}");
        assert_eq!(image::open(resolve(&nombre)).unwrap().width(), 256);
    }

    #[test]
    fn un_gif_que_pasa_del_tope_se_queda_en_el_primer_fotograma() {
        // El tope existe porque un GIF animado entra con las dimensiones que traiga
        // --no se puede reescalar sin recodificarlo-- y el webview lo decodifica en
        // cada vuelta. Se prueba la decision sola para no fabricar dos megas.
        assert_eq!(decidir(false, 12, MAX_GIF), Some("gif"));
        assert_eq!(decidir(false, 12, MAX_GIF + 1), None);
    }

    #[test]
    fn un_svg_embebido_en_un_perfil_llega_entero_al_otro_equipo() {
        // Esto estaba roto: `import_bytes` solo sabia escribir PNG, asi que al
        // importar un perfil exportado la imagen SVG se descartaba en silencio y la
        // tecla llegaba sin icono.
        let (_dir, _g) = aislar("svgbytes");
        let contenido = br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="8" height="8"/></svg>"#;

        let nombre = import_bytes(contenido).unwrap();
        assert!(nombre.ends_with(".svg"), "{nombre}");
        assert_eq!(fs::read(resolve(&nombre)).unwrap(), contenido);
    }

    #[test]
    fn un_gif_embebido_en_un_perfil_llega_animado() {
        let (dir, _g) = aislar("gifbytes");
        let bytes = fs::read(escribir_gif(&dir, "late.gif", 3, 64)).unwrap();

        let nombre = import_bytes(&bytes).unwrap();
        assert!(nombre.ends_with(".gif"), "{nombre}");
        assert_eq!(fs::read(resolve(&nombre)).unwrap(), bytes);
    }

    #[test]
    fn un_archivo_que_no_es_imagen_no_ensucia_la_biblioteca() {
        let (dir, _g) = aislar("basura");
        let src = dir.join("documento.txt");
        fs::write(&src, "esto no es una imagen").unwrap();

        assert!(matches!(
            import(&src),
            Err(ImageError::FormatoNoSoportado(_))
        ));
        assert_eq!(fs::read_dir(library_dir()).unwrap().flatten().count(), 0);
    }

    #[test]
    fn un_png_con_extension_valida_pero_contenido_roto_da_error() {
        let (dir, _g) = aislar("roto");
        let src = dir.join("mentira.png");
        fs::write(&src, b"PNG de mentira").unwrap();

        assert!(matches!(import(&src), Err(ImageError::NoSePudoLeer(_))));
    }

    #[test]
    fn detecta_las_imagenes_que_ninguna_tecla_usa() {
        let (dir, _g) = aislar("sinuso");
        let usada = import(&escribir_png(&dir, "usada.png", 32, 32, 255)).unwrap();
        let suelta = import(&escribir_png(&dir, "suelta.png", 48, 48, 255)).unwrap();

        let mut deck = crate::store::default_deck();
        let raiz = deck.surfaces.get_mut("s-root").unwrap();
        raiz.pages[0].buttons[0].icon.source = IconSource::Image {
            file: usada.clone(),
        };

        let sueltas = unused(&deck);
        assert_eq!(sueltas.len(), 1);
        assert_eq!(sueltas[0].file, suelta);
        assert!(sueltas[0].bytes > 0);
    }

    #[test]
    fn soltar_una_imagen_da_una_tecla_que_muestra_esa_imagen() {
        use crate::model::IconSource;
        let (dir, _g) = aislar("icono_para");
        let src = escribir_png(&dir, "logo.png", 120, 120, 255);

        let icono = icono_para(&src);
        let IconSource::Image { file } = &icono.source else {
            panic!(
                "una imagen soltada debe quedar como cara de la tecla, no como icono automatico"
            );
        };
        assert!(resolve(file).exists());
    }

    #[test]
    fn soltar_algo_que_no_es_imagen_deja_el_icono_automatico() {
        use crate::model::IconSource;
        let (dir, _g) = aislar("icono_para_exe");
        let src = dir.join("programa.exe");
        fs::write(&src, b"MZ").unwrap();
        assert!(matches!(icono_para(&src).source, IconSource::Auto));
    }

    #[test]
    fn borrar_no_acepta_nombres_que_escapen_de_la_biblioteca() {
        let _g = aislar("escape");
        assert!(delete("..\\..\\deck.json").is_err());
        assert!(delete("sub/otro.png").is_err());
    }
}
