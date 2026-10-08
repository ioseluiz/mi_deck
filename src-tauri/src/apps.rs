//! Aplicaciones instaladas, para elegirlas por nombre en vez de por ruta.
//!
//! Pedirle a alguien la ruta de un `.exe` es pedirle que sepa donde instala cada
//! fabricante, que no es razonable. La lista sale del **Menu Inicio**, que es
//! justo lo que esa persona ya ve cuando pulsa el boton de Windows: un `.lnk` por
//! aplicacion, con el nombre que el fabricante eligio.
//!
//! Se eligio el Menu Inicio y no el registro de desinstalacion (`Uninstall`)
//! porque ese enumera paquetes, no aplicaciones: salen actualizaciones,
//! redistribuibles y componentes que nadie quiere en una tecla, y la mitad no
//! tiene con que lanzarse.
//!
//! Limitacion conocida: las aplicaciones de la Tienda que no dejan acceso directo
//! en el Menu Inicio no aparecen. Enumerarlas exige recorrer `shell:AppsFolder`
//! por COM, que es otro proyecto; para esas sigue estando el campo de ruta.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Hasta donde se baja en el arbol del Menu Inicio. Las carpetas de fabricante
/// rara vez pasan de dos niveles; el limite esta para que un arbol enfermo no
/// deje el editor pensando.
const PROFUNDIDAD_MAXIMA: usize = 5;

/// Tope de resultados. Un desplegable con mil entradas no es una ayuda.
const MAXIMO: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInstalada {
    /// Lo que el usuario reconoce: el nombre del acceso directo.
    pub nombre: String,
    /// Ruta del `.lnk`, que es la que se guarda en la tecla.
    pub ruta: String,
}

/// Las dos raices del Menu Inicio: la de la maquina y la del usuario.
fn menus_inicio() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for (variable, resto) in [
        ("ProgramData", r"Microsoft\Windows\Start Menu\Programs"),
        ("APPDATA", r"Microsoft\Windows\Start Menu\Programs"),
    ] {
        if let Ok(base) = std::env::var(variable) {
            let p = PathBuf::from(base).join(resto);
            if p.is_dir() {
                v.push(p);
            }
        }
    }
    v
}

/// Un acceso directo lanzable. `.url` entra porque muchos portales corporativos
/// se instalan asi y para una tecla valen igual que un `.lnk`.
pub fn es_atajo(nombre: &str) -> bool {
    let bajo = nombre.to_ascii_lowercase();
    bajo.ends_with(".lnk") || bajo.ends_with(".url")
}

/// Entradas que el Menu Inicio trae pero que nadie quiere en una tecla.
///
/// Se filtran a proposito y no por pereza de mostrarlas: una tecla llamada
/// "Desinstalar" junto a las demas es un accidente esperando. Quien busque una de
/// estas siempre puede poner la ruta a mano.
pub fn es_ruido(nombre: &str) -> bool {
    let bajo = nombre.to_ascii_lowercase();
    [
        "uninstall",
        "desinstalar",
        "readme",
        "leame",
        "release notes",
        "notas de la version",
        "help",
        "ayuda",
        "documentation",
        "documentacion",
        "website",
        "sitio web",
    ]
    .iter()
    .any(|m| bajo.contains(m))
}

/// Nombre visible de un acceso directo: el del archivo sin extension.
pub fn nombre_visible(archivo: &str) -> String {
    match archivo.rfind('.') {
        Some(i) if i > 0 => archivo[..i].to_string(),
        _ => archivo.to_string(),
    }
}

/// Ordena por nombre y quita repetidos.
///
/// El mismo programa suele estar en el Menu Inicio de la maquina y en el del
/// usuario. Se compara sin distinguir mayusculas porque "Teams" y "teams" son la
/// misma entrada para quien mira la lista.
pub fn ordenar_y_deduplicar(mut v: Vec<AppInstalada>) -> Vec<AppInstalada> {
    v.sort_by(|a, b| {
        a.nombre
            .to_lowercase()
            .cmp(&b.nombre.to_lowercase())
            .then_with(|| a.ruta.cmp(&b.ruta))
    });
    v.dedup_by(|a, b| a.nombre.eq_ignore_ascii_case(&b.nombre));
    v.truncate(MAXIMO);
    v
}

fn recolectar(dir: &Path, profundidad: usize, salida: &mut Vec<AppInstalada>) {
    if profundidad > PROFUNDIDAD_MAXIMA || salida.len() >= MAXIMO * 4 {
        return;
    }
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return; // Una carpeta sin permiso no debe tumbar la lista entera.
    };

    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        let Some(archivo) = ruta.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if ruta.is_dir() {
            recolectar(&ruta, profundidad + 1, salida);
        } else if es_atajo(archivo) {
            let nombre = nombre_visible(archivo);
            if nombre.trim().is_empty() || es_ruido(&nombre) {
                continue;
            }
            salida.push(AppInstalada {
                nombre,
                ruta: ruta.to_string_lossy().to_string(),
            });
        }
    }
}

/// Las aplicaciones del Menu Inicio, ordenadas y sin repetidos.
pub fn listar() -> Vec<AppInstalada> {
    let mut v = Vec::new();
    for raiz in menus_inicio() {
        recolectar(&raiz, 0, &mut v);
    }
    ordenar_y_deduplicar(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(nombre: &str, ruta: &str) -> AppInstalada {
        AppInstalada {
            nombre: nombre.into(),
            ruta: ruta.into(),
        }
    }

    #[test]
    fn reconoce_los_accesos_directos() {
        assert!(es_atajo("Excel.lnk"));
        assert!(es_atajo("INTRANET.URL"));
        assert!(!es_atajo("notas.txt"));
        assert!(!es_atajo("Excel"));
    }

    #[test]
    fn el_nombre_visible_quita_la_extension() {
        assert_eq!(nombre_visible("Microsoft Excel.lnk"), "Microsoft Excel");
        assert_eq!(
            nombre_visible("Visual Studio 2022.lnk"),
            "Visual Studio 2022"
        );
        // Un nombre con puntos conserva todos menos el ultimo.
        assert_eq!(nombre_visible("Node.js 20.lnk"), "Node.js 20");
        assert_eq!(nombre_visible("sin_extension"), "sin_extension");
    }

    #[test]
    fn se_descartan_las_entradas_que_nadie_pondria_en_una_tecla() {
        assert!(es_ruido("Uninstall Zoom"));
        assert!(es_ruido("Desinstalar SAP"));
        assert!(es_ruido("Readme"));
        assert!(es_ruido("Documentacion de Python"));
    }

    #[test]
    fn una_aplicacion_normal_no_se_descarta() {
        for n in ["Microsoft Excel", "Google Chrome", "AutoCAD 2024", "Teams"] {
            assert!(!es_ruido(n), "{n} no deberia descartarse");
        }
    }

    #[test]
    fn el_mismo_programa_en_los_dos_menus_sale_una_sola_vez() {
        let v = ordenar_y_deduplicar(vec![
            app("Teams", r"C:\ProgramData\...\Teams.lnk"),
            app("teams", r"C:\Users\x\...\Teams.lnk"),
            app("Excel", r"C:\ProgramData\...\Excel.lnk"),
        ]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].nombre, "Excel");
    }

    #[test]
    fn la_lista_sale_ordenada_por_nombre_sin_distinguir_mayusculas() {
        let v = ordenar_y_deduplicar(vec![
            app("zoom", "z.lnk"),
            app("Acrobat", "a.lnk"),
            app("mIRC", "m.lnk"),
        ]);
        let nombres: Vec<&str> = v.iter().map(|a| a.nombre.as_str()).collect();
        assert_eq!(nombres, vec!["Acrobat", "mIRC", "zoom"]);
    }

    #[test]
    fn la_lista_no_crece_sin_limite() {
        let muchas: Vec<AppInstalada> = (0..MAXIMO + 50)
            .map(|i| app(&format!("App {i:04}"), &format!("{i}.lnk")))
            .collect();
        assert_eq!(ordenar_y_deduplicar(muchas).len(), MAXIMO);
    }
}
