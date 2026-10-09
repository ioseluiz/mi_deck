//! Exportar e importar un perfil como un archivo que se puede pasar a alguien.
//!
//! Un perfil es trabajo: averiguar los atajos de una aplicacion, probarlos y
//! ordenarlos lleva una tarde. Sin esto, cada persona de un equipo repite esa
//! tarde entera, y no hay forma de llevarse el trabajo a otro equipo.
//!
//! El archivo lleva **un perfil**, no el deck: al importarlo se anade a lo que ya
//! tengas sin tocar nada mas. Un formato de deck completo serviria de respaldo
//! pero no para compartir, que es lo que hace falta.

use crate::model::{Deck, DeckButton, IconSource, Page, Surface};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Version del formato. Si algun dia cambia, un archivo viejo se reconoce.
pub const VERSION: u32 = 1;

/// Un perfil listo para viajar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfilExportado {
    pub version: u32,
    /// Nombre del panel, que es como se reconoce el perfil.
    pub nombre: String,
    /// Ejecutables que cubre.
    #[serde(default)]
    pub exes: Vec<String>,
    /// El panel entero, con sus paginas y teclas.
    pub panel: Surface,
    /// Los paneles de las carpetas a las que apuntan sus teclas.
    #[serde(default)]
    pub carpetas: HashMap<String, Surface>,
    /// Imagenes propias usadas por las teclas, por nombre de archivo y en base64.
    ///
    /// Van dentro para que quien reciba el archivo vea el perfil tal cual, sin
    /// tener que pedir nada mas. Es lo que lo hace pesar, y es el precio de que
    /// funcione a la primera.
    #[serde(default)]
    pub imagenes: HashMap<String, String>,
}

/// Recoge un perfil del deck en una pieza suelta.
///
/// `leer_imagen` entra por parametro para que la funcion se pueda probar sin
/// tocar el disco: el unico contacto con el sistema es ese.
pub fn exportar(
    deck: &Deck,
    profile_id: &str,
    leer_imagen: impl Fn(&str) -> Option<Vec<u8>>,
) -> Result<PerfilExportado, String> {
    let perfil = deck
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("No existe el perfil {profile_id}"))?;

    let panel = deck
        .surfaces
        .get(&perfil.surface)
        .ok_or_else(|| "El perfil apunta a un panel que no existe.".to_string())?
        .clone();

    // Las carpetas del perfil van con el, o al importarlo las teclas de carpeta
    // apuntarian al vacio.
    let mut carpetas = HashMap::new();
    recoger_carpetas(deck, &panel, &mut carpetas);

    let mut imagenes = HashMap::new();
    for archivo in imagenes_usadas(&panel, &carpetas) {
        if let Some(bytes) = leer_imagen(&archivo) {
            imagenes.insert(
                archivo,
                base64::engine::general_purpose::STANDARD.encode(bytes),
            );
        }
    }

    Ok(PerfilExportado {
        version: VERSION,
        nombre: panel.name.clone(),
        exes: perfil.exes.clone(),
        panel,
        carpetas,
        imagenes,
    })
}

/// Mete un perfil exportado en el deck. Devuelve el identificador del nuevo.
///
/// `guardar_imagen` recibe los bytes ya descodificados y devuelve el nombre con
/// el que quedaron guardados, que no tiene por que ser el de origen: la
/// biblioteca nombra por contenido, asi que dos personas con la misma imagen
/// acaban compartiendo archivo.
pub fn importar(
    deck: &mut Deck,
    paquete: &PerfilExportado,
    guardar_imagen: impl Fn(&[u8]) -> Option<String>,
) -> Result<String, String> {
    if paquete.version > VERSION {
        return Err(format!(
            "Este archivo es de una version mas nueva de MiDeck (formato {}). \
             Actualiza para poder abrirlo.",
            paquete.version
        ));
    }
    if paquete.exes.is_empty() {
        return Err("El archivo no dice para que aplicacion es.".to_string());
    }

    // Un ejecutable que ya tiene perfil no se puede robar: ganaria el primero y
    // el importado no se activaria nunca. Es mejor no dejar entrar algo muerto.
    for exe in &paquete.exes {
        if let Some(ya) = crate::perfiles::perfil_para(&deck.profiles, exe) {
            let como = deck
                .surfaces
                .get(&ya.surface)
                .map(|s| s.name.as_str())
                .unwrap_or("otro");
            return Err(format!("Ya hay un perfil para {exe}: «{como}»."));
        }
    }

    // Las imagenes primero: hay que saber su nombre nuevo antes de reescribir las
    // teclas que las usan.
    let mut renombradas: HashMap<String, String> = HashMap::new();
    for (nombre, datos) in &paquete.imagenes {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(datos) else {
            continue;
        };
        if let Some(nuevo) = guardar_imagen(&bytes) {
            renombradas.insert(nombre.clone(), nuevo);
        }
    }

    // Identificadores nuevos para los paneles, para no chocar con los del deck.
    let mut mapa: HashMap<String, String> = HashMap::new();
    let principal = crate::edit::nuevo_id(deck, "s");
    mapa.insert("__principal__".to_string(), principal.clone());
    for viejo in paquete.carpetas.keys() {
        mapa.insert(viejo.clone(), crate::edit::nuevo_id(deck, "s"));
    }

    let insertar = |deck: &mut Deck, id: &str, origen: &Surface| {
        let mut copia = Surface::new(&origen.name);
        copia.pages = origen
            .pages
            .iter()
            .map(|pagina| Page {
                buttons: pagina
                    .buttons
                    .iter()
                    .map(|b| {
                        let mut nuevo = b.clone();
                        nuevo.id = crate::edit::nuevo_id(deck, "b");
                        if let crate::model::Action::Folder { surface } = &nuevo.action {
                            if let Some(destino) = mapa.get(surface) {
                                nuevo.action = crate::model::Action::Folder {
                                    surface: destino.clone(),
                                };
                            }
                        }
                        if let IconSource::Image { file } = &nuevo.icon.source {
                            if let Some(nombre) = renombradas.get(file) {
                                nuevo.icon.source = IconSource::Image {
                                    file: nombre.clone(),
                                };
                            }
                        }
                        nuevo
                    })
                    .collect(),
            })
            .collect();
        deck.surfaces.insert(id.to_string(), copia);
    };

    insertar(deck, &principal, &paquete.panel);
    for (viejo, panel) in &paquete.carpetas {
        let id = mapa[viejo].clone();
        insertar(deck, &id, panel);
    }

    let id = crate::edit::nuevo_id(deck, "p");
    deck.profiles.push(crate::perfiles::Profile {
        id: id.clone(),
        surface: principal,
        exes: paquete
            .exes
            .iter()
            .map(|e| crate::focus::nombre_de_ejecutable(e))
            .collect(),
        enabled: true,
    });
    Ok(id)
}

/// Nombre de archivo sugerido, sin caracteres que Windows rechace.
pub fn nombre_de_archivo(nombre: &str) -> String {
    let limpio: String = nombre
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let limpio = limpio.trim_matches('-').to_lowercase();
    if limpio.is_empty() {
        "mideck-perfil.json".to_string()
    } else {
        format!("mideck-{limpio}.json")
    }
}

// ------------------------------------------------------------------ interiores

/// Mete en `fuera` los paneles de las carpetas a las que llega este panel.
fn recoger_carpetas(deck: &Deck, panel: &Surface, fuera: &mut HashMap<String, Surface>) {
    for boton in panel.pages.iter().flat_map(|p| p.buttons.iter()) {
        let crate::model::Action::Folder { surface } = &boton.action else {
            continue;
        };
        if fuera.contains_key(surface) {
            // Ya recogida: ademas de no duplicarla, corta un ciclo de carpetas
            // que se apunten entre si.
            continue;
        }
        let Some(hija) = deck.surfaces.get(surface) else {
            continue;
        };
        fuera.insert(surface.clone(), hija.clone());
        recoger_carpetas(deck, hija, fuera);
    }
}

/// Archivos de imagen que usan las teclas de un perfil.
fn imagenes_usadas(panel: &Surface, carpetas: &HashMap<String, Surface>) -> Vec<String> {
    let mut v: Vec<String> = std::iter::once(panel)
        .chain(carpetas.values())
        .flat_map(|s| s.pages.iter())
        .flat_map(|p| p.buttons.iter())
        .filter_map(|b: &DeckButton| match &b.icon.source {
            IconSource::Image { file } => Some(file.clone()),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::create_profile;
    use crate::model::{Action, Icon};
    use crate::store::default_deck;

    fn tecla(id: &str, pos: u32, accion: Action, icono: IconSource) -> DeckButton {
        DeckButton {
            id: id.into(),
            position: pos,
            label: id.into(),
            icon: Icon {
                source: icono,
                ..Icon::default()
            },
            action: accion,
            wheel: None,
            extra: Default::default(),
        }
    }

    /// Deck con un perfil de Excel que tiene una tecla con imagen propia y una
    /// carpeta con otra tecla dentro.
    fn deck_con_perfil() -> (Deck, String) {
        let mut deck = default_deck();
        let sid = create_profile(&mut deck, "excel.exe", "Excel").unwrap();
        let id = deck.profiles[0].id.clone();

        deck.surfaces
            .insert("s-sub".into(), Surface::new("Pegado especial"));
        deck.surfaces.get_mut("s-sub").unwrap().pages = vec![Page {
            buttons: vec![tecla(
                "b-dentro",
                1,
                Action::Hotkey {
                    keys: "Ctrl+Alt+V".into(),
                },
                IconSource::Builtin {
                    name: "rayo".into(),
                },
            )],
        }];

        deck.surfaces.get_mut(&sid).unwrap().pages = vec![Page {
            buttons: vec![
                tecla(
                    "b-img",
                    0,
                    Action::Hotkey {
                        keys: "Ctrl+S".into(),
                    },
                    IconSource::Image {
                        file: "abc123.png".into(),
                    },
                ),
                tecla(
                    "b-carpeta",
                    1,
                    Action::Folder {
                        surface: "s-sub".into(),
                    },
                    IconSource::Auto,
                ),
            ],
        }];
        (deck, id)
    }

    #[test]
    fn un_perfil_exportado_lleva_su_carpeta_y_su_imagen() {
        let (deck, id) = deck_con_perfil();
        let p = exportar(&deck, &id, |_| Some(vec![1, 2, 3])).unwrap();

        assert_eq!(p.nombre, "Excel");
        assert_eq!(p.exes, vec!["excel.exe"]);
        assert_eq!(p.carpetas.len(), 1, "la carpeta tiene que viajar con el");
        assert_eq!(
            p.imagenes.get("abc123.png").map(String::as_str),
            Some("AQID"),
            "la imagen va dentro, en base64"
        );
    }

    #[test]
    fn una_imagen_que_no_esta_en_disco_no_rompe_la_exportacion() {
        // Mejor un perfil sin esa imagen que no poder compartirlo.
        let (deck, id) = deck_con_perfil();
        let p = exportar(&deck, &id, |_| None).unwrap();
        assert!(p.imagenes.is_empty());
    }

    #[test]
    fn ida_y_vuelta_a_otro_deck_deja_el_perfil_entero() {
        let (origen, id) = deck_con_perfil();
        let paquete = exportar(&origen, &id, |_| Some(vec![1, 2, 3])).unwrap();

        let mut destino = default_deck();
        let nuevo = importar(&mut destino, &paquete, |_| Some("xyz789.png".to_string())).unwrap();

        let perfil = destino.profiles.iter().find(|p| p.id == nuevo).unwrap();
        let panel = &destino.surfaces[&perfil.surface];
        assert_eq!(panel.name, "Excel");
        assert_eq!(perfil.exes, vec!["excel.exe"]);
        assert_eq!(panel.pages[0].buttons.len(), 2);

        // La imagen quedo guardada con el nombre que dijo la biblioteca, y la
        // tecla apunta a ese y no al del equipo de origen.
        let IconSource::Image { file } = &panel.pages[0].buttons[0].icon.source else {
            panic!("la primera tecla deberia tener imagen propia");
        };
        assert_eq!(file, "xyz789.png");

        // Y la carpeta apunta a un panel que existe en el deck de destino.
        let Action::Folder { surface } = &panel.pages[0].buttons[1].action else {
            panic!("la segunda tecla deberia ser una carpeta");
        };
        assert!(
            destino.surfaces.contains_key(surface),
            "la carpeta importada apunta al vacio"
        );
        assert_eq!(destino.surfaces[surface].pages[0].buttons.len(), 1);
    }

    #[test]
    fn importar_no_reutiliza_identificadores_del_deck_que_lo_recibe() {
        // Dos importaciones del mismo archivo no pueden pisarse entre si.
        let (origen, id) = deck_con_perfil();
        let mut paquete = exportar(&origen, &id, |_| None).unwrap();

        let mut destino = default_deck();
        importar(&mut destino, &paquete, |_| None).unwrap();
        paquete.exes = vec!["winword.exe".into()];
        importar(&mut destino, &paquete, |_| None).unwrap();

        let s1 = &destino.profiles[0].surface;
        let s2 = &destino.profiles[1].surface;
        assert_ne!(s1, s2, "los dos perfiles comparten panel");
    }

    #[test]
    fn no_entra_un_perfil_para_una_aplicacion_que_ya_tiene_uno() {
        // Ganaria el que ya estaba y el importado no se activaria nunca: dejarlo
        // entrar seria dar trabajo por hecho que no funciona.
        let (origen, id) = deck_con_perfil();
        let paquete = exportar(&origen, &id, |_| None).unwrap();

        let (mut destino, _) = deck_con_perfil();
        let Err(motivo) = importar(&mut destino, &paquete, |_| None) else {
            panic!("deberia haberse quejado");
        };
        assert!(motivo.contains("excel.exe"), "{motivo}");
    }

    #[test]
    fn un_archivo_de_una_version_mas_nueva_se_rechaza_con_su_motivo() {
        let (origen, id) = deck_con_perfil();
        let mut paquete = exportar(&origen, &id, |_| None).unwrap();
        paquete.version = VERSION + 1;

        let mut destino = default_deck();
        let Err(motivo) = importar(&mut destino, &paquete, |_| None) else {
            panic!("deberia haberse quejado");
        };
        assert!(motivo.contains("Actualiza"), "{motivo}");
    }

    #[test]
    fn el_nombre_del_archivo_es_valido_en_windows() {
        assert_eq!(nombre_de_archivo("Excel"), "mideck-excel.json");
        assert_eq!(
            nombre_de_archivo("Revit / Estructuras"),
            "mideck-revit---estructuras.json"
        );
        // Un nombre entero de caracteres raros no puede dar un archivo sin nombre.
        assert_eq!(nombre_de_archivo("///"), "mideck-perfil.json");
    }

    #[test]
    fn unas_carpetas_que_se_apuntan_entre_si_no_cuelgan_la_exportacion() {
        let (mut deck, id) = deck_con_perfil();
        let sid = deck.profiles[0].surface.clone();
        // La subcarpeta vuelve al panel del perfil: un ciclo.
        deck.surfaces.get_mut("s-sub").unwrap().pages[0]
            .buttons
            .push(tecla(
                "b-vuelta",
                2,
                Action::Folder {
                    surface: sid.clone(),
                },
                IconSource::Auto,
            ));

        let p = exportar(&deck, &id, |_| None).unwrap();
        assert!(p.carpetas.contains_key("s-sub"));
    }
}
